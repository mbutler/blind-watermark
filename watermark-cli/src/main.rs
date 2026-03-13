use clap::{Parser, Subcommand};
use image::ImageReader;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use p256::SecretKey;
use std::fs;
use watermark_core::embed_manager::WatermarkEngine;
use watermark_core::image_manager::YCbCrImage;
use watermark_core::schema::ProvenancePayload;
use watermark_core::PayloadManager;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Embed {
        #[arg(short, long)]
        input: String,
        #[arg(short, long)]
        output: String,
        #[arg(short, long)]
        key_path: String,
        #[arg(short, long)]
        asset_id: u32,
        /// The quantization step size (higher = more robust, but more visible)
        #[arg(short = 's', long, default_value_t = 30.0)]
        step_size: f64,
    },
    Extract {
        #[arg(short, long)]
        input: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // 38 data bytes, 20 parity bytes = 58 bytes total
    let payload_manager = PayloadManager::with_params(38, 20).expect("Failed to init FEC");

    match &cli.command {
        Commands::Embed {
            input,
            output,
            key_path,
            asset_id,
            step_size,
        } => {
            let engine = WatermarkEngine::new(*step_size);
            println!("Reading private key from vault...");
            let pem_str = fs::read_to_string(key_path).expect("Failed to read PEM file");
            let secret_key =
                SecretKey::from_pkcs8_pem(&pem_str).expect("Failed to decode PKCS#8");

            // Derive the 33-byte compressed public key
            let encoded = secret_key.public_key().to_encoded_point(true);
            let compressed_bytes = encoded.as_bytes();
            let mut pubkey_array = [0u8; 33];
            pubkey_array.copy_from_slice(compressed_bytes);

            // Construct our strict binary payload
            let provenance = ProvenancePayload {
                version: 1,
                compressed_pubkey: pubkey_array,
                asset_id: *asset_id,
            };

            let raw_bytes = provenance.to_bytes();

            println!("Encoding payload with Reed-Solomon...");
            let robust_payload = payload_manager.encode_payload(&raw_bytes).unwrap();

            println!("Loading image from {}...", input);
            let img = ImageReader::open(input).unwrap().decode().unwrap().to_rgb8();
            let mut ycbcr = YCbCrImage::from_rgb(&img);

            println!("Embedding into DCT frequency domain...");
            engine.embed(
                &mut ycbcr.y_channel,
                ycbcr.width as usize,
                ycbcr.height as usize,
                &robust_payload,
            );

            println!("Reconstructing and saving to {}...", output);
            ycbcr.to_rgb().save(output).unwrap();

            println!("Done! Embedded Asset ID: {}", asset_id);
        }
        Commands::Extract { input } => {
            println!("Loading image from {}...", input);
            let img = ImageReader::open(input).unwrap().decode().unwrap().to_rgb8();
            let ycbcr = YCbCrImage::from_rgb(&img);
            let width = ycbcr.width as usize;
            let height = ycbcr.height as usize;
            let expected_bytes = payload_manager.total_payload_bytes();

            const STEP_SIZES: [f64; 6] = [25.0, 30.0, 35.0, 40.0, 45.0, 50.0];
            println!("Probing step sizes {:?}...", STEP_SIZES);

            let mut found = false;
            for &step_size in &STEP_SIZES {
                let engine = WatermarkEngine::new(step_size);
                let extracted = engine.extract(&ycbcr.y_channel, width, height, expected_bytes);

                if let Ok(raw_bytes) = payload_manager.decode_payload(&extracted) {
                    if let Ok(provenance) = ProvenancePayload::from_bytes(&raw_bytes) {
                        if provenance.version == 1 {
                            println!("\nSUCCESS! Valid Provenance Found (S={}):", step_size);
                            println!("Version: {}", provenance.version);
                            println!("Asset ID: {}", provenance.asset_id);
                            println!("Public Key (hex): {}", hex::encode(provenance.compressed_pubkey));
                            found = true;
                            break;
                        }
                    }
                }
            }

            if !found {
                println!("\nFAILED: No valid provenance found at any step size.");
            }
        }
    }
}

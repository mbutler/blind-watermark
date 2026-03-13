use clap::{Parser, Subcommand};
use image::ImageReader;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use p256::SecretKey;
use std::fs;
use watermark_core::{apply_watermark, extract_watermark, ProvenancePayload};

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

    match &cli.command {
        Commands::Embed {
            input,
            output,
            key_path,
            asset_id,
            step_size,
        } => {
            println!("Reading private key from vault...");
            let pem_str = fs::read_to_string(key_path).expect("Failed to read PEM file");
            let secret_key =
                SecretKey::from_pkcs8_pem(&pem_str).expect("Failed to decode PKCS#8");

            let encoded = secret_key.public_key().to_encoded_point(true);
            let compressed_bytes = encoded.as_bytes();
            let mut pubkey_array = [0u8; 33];
            pubkey_array.copy_from_slice(compressed_bytes);

            let provenance = ProvenancePayload {
                version: 1,
                compressed_pubkey: pubkey_array,
                asset_id: *asset_id,
                checksum: 0, // Computed in to_bytes()
            };

            println!("Loading image from {}...", input);
            let mut img = ImageReader::open(input).unwrap().decode().unwrap().to_rgb8();

            apply_watermark(&mut img, &provenance, *step_size).expect("Failed to apply watermark");

            println!("Reconstructing and saving to {}...", output);
            img.save(output).unwrap();

            println!("Done! Embedded Asset ID: {}", asset_id);
        }
        Commands::Extract { input } => {
            println!("Loading image from {}...", input);
            let img = ImageReader::open(input).unwrap().decode().unwrap().to_rgb8();

            match extract_watermark(&img) {
                Ok((provenance, step_size)) => {
                    println!("\nSUCCESS! Valid Provenance Found (S={}):", step_size);
                    println!("Version: {}", provenance.version);
                    println!("Asset ID: {}", provenance.asset_id);
                    println!("Public Key (hex): {}", hex::encode(provenance.compressed_pubkey));
                }
                Err(e) => {
                    println!("\nFAILED: {}", e);
                }
            }
        }
    }
}

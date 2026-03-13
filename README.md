# blind-watermark

A blind, compression-resistant, cryptographically verifiable provenance watermark engine written in Rust. Embeds creator identity (P-256 public key) and asset metadata into images using a hybrid DWT-DCT pipeline with Quantization Index Modulation (QIM). The watermark survives JPEG compression and requires no original image for extraction.

## Features

- **Blind extraction** — No original image needed; auto-discovers step size
- **JPEG-resistant** — Survives 75% quality JPEG compression at step size S=45
- **Cryptographically verifiable** — Embeds P-256 compressed public key (33 bytes)
- **Reed-Solomon FEC** — 42 data + 16 parity = 58 bytes; recovers from up to 16 corrupted bytes
- **Spatial repetition** — Payload looped across the HL band; large images hold ~40–50 redundant copies
- **Hybrid DWT-DCT** — Watermark embedded in HL (horizontal edge) sub-band for invisibility

## How It Works

### Pipeline Architecture

```
Embed:  RGB → YCbCr → DWT → HL band → 8×8 DCT → QIM embed → IDCT → IDWT → RGB
Extract: RGB → YCbCr → DWT → HL band → 8×8 DCT → QIM extract → Reed-Solomon decode → Schema parse
```

1. **Color space** — Convert RGB to YCbCr (ITU-R BT.601). Only the Y (luminance) channel is modified; Cb/Cr pass through untouched.

2. **DWT (Discrete Wavelet Transform)** — 1-level 2D Haar splits the image into four sub-bands:
   - **LL** (top-left): Blurry miniature — left alone (too sensitive)
   - **HL** (top-right): Horizontal edges — **embed here** (sweet spot)
   - **LH** (bottom-left): Vertical edges
   - **HH** (bottom-right): High-frequency noise — left alone (JPEG destroys it)

3. **DCT (Discrete Cosine Transform)** — 8×8 blocks in the HL band, same domain JPEG uses. Coefficient 27 (row 3, col 3) is used — mid-frequency for robustness vs. visibility.

4. **QIM (Quantization Index Modulation)** — Embed bit 0: snap to even multiples of step size S. Embed bit 1: snap to odd multiples. Extracts by bucket (even→0, odd→1). Erasure signaling: if a coefficient drifts too close to a boundary, mark as `None` for Reed-Solomon.

5. **Reed-Solomon** — 42 data shards + 16 parity shards = 58 bytes total. Corrects up to 16 erasures.

6. **Binary schema** — 42-byte payload: `version` (1) + `compressed_pubkey` (33) + `asset_id` (4) + `CRC32` (4). CRC32 validates integrity; spatial repetition loops the payload across the HL band for redundancy.

### Real Estate

The HL band is ¼ of the original pixels. Each 8×8 block embeds 1 bit. With **spatial repetition**, the payload is stamped across the entire HL band (loops via `block_index % 464`); larger images hold many redundant copies (e.g. 1080p ≈ 40–50 copies). The **original image must be at least ~344×344 pixels** for extraction to have one full 464-block chunk. Extraction tries each chunk until CRC32 validates.

## Requirements

### Key Format

A P-256 (secp256r1) private key in **PKCS#8 PEM** format. Generate with:

```bash
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out key.pem
```

Traditional `EC PRIVATE KEY` format will not work; convert with:

```bash
openssl pkcs8 -topk8 -nocrypt -in ec_key.pem -out key.pem
```

## CLI

### Embed

Embeds provenance (public key + asset ID) into an image.

```bash
cargo run --bin watermark-cli -- embed -i <input> -o <output> -k <key.pem> --asset-id <id> [-s <step_size>]
```

| Option | Short | Description |
|--------|-------|-------------|
| `--input` | `-i` | Path to source image (JPEG, PNG, etc.) |
| `--output` | `-o` | Path to save watermarked image |
| `--key-path` | `-k` | Path to P-256 PKCS#8 PEM file |
| `--asset-id` | | Unsigned 32-bit asset identifier |
| `--step-size` | `-s` | QIM quantization step (default: 30.0). Higher = more robust to compression, more visible. Use 45 for JPEG 75% survival. |

**Example:**

```bash
cargo run --bin watermark-cli -- embed -i photo.jpg -o watermarked.png -k key.pem --asset-id 999 -s 45
```

### Extract

Extracts provenance from an image. **No step size needed** — auto-probes `[25, 30, 35, 40, 45, 50, 55]` and returns the first valid result.

```bash
cargo run --bin watermark-cli -- extract -i <input>
```

| Option | Short | Description |
|--------|-------|-------------|
| `--input` | `-i` | Path to watermarked image |

**Example:**

```bash
cargo run --bin watermark-cli -- extract -i compressed.jpg
```

**Output (success):**

```
Loading image from compressed.jpg...
Probing step sizes [25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0]...
SUCCESS! Valid Provenance Found (S=45.0):
Version: 1
Asset ID: 999
Public Key (hex): 021dce54e3ec92e10bd0eaa6b47bd5a354ad7007737990fe3983f6ed98f5113fac
```

## JPEG Crucible Test

Quick manual test:

```bash
# 1. Embed
cargo run --bin watermark-cli -- embed -i original.jpg -o watermarked.png -k key.pem --asset-id 999 -s 45

# 2. Compress (ImageMagick)
magick watermarked.png -quality 75 compressed.jpg

# 3. Extract
cargo run --bin watermark-cli -- extract -i compressed.jpg
```

If extraction succeeds, the watermark survives JPEG 75%.

### Full Crucible (Aggressive End-to-End)

Runs 8 torture tests: PNG baseline, JPEG 75%/85%/95%, crop 400×400, crop 360×360, crop+JPEG, resize 70%.

```bash
./scripts/crucible.sh <image.jpg>
```

Requires ImageMagick (`magick` or `convert`). **Baseline:** A (PNG) and F (JPEG 95%) must pass. Crop and resize tests may fail due to DWT boundary effects and interpolation—this documents known limits.

## Building & Testing

```bash
# Build
cargo build

# Run all tests
cargo test

# Run CLI help
cargo run --bin watermark-cli -- --help
cargo run --bin watermark-cli -- embed --help
cargo run --bin watermark-cli -- extract --help
```

## Project Structure

```
blind-watermark/
├── watermark-core/     # Library
│   ├── lib.rs          # PayloadManager (Reed-Solomon)
│   ├── schema.rs       # ProvenancePayload (42-byte: 38 data + 4 CRC32)
│   ├── image_manager.rs # YCbCr conversion
│   ├── dwt_manager.rs   # 2D Haar wavelet
│   ├── transform_manager.rs # 8×8 DCT
│   ├── qim_manager.rs   # QIM embed/extract with erasure
│   └── embed_manager.rs # DWT→HL→DCT-QIM orchestration
└── watermark-cli/      # CLI frontend
```

## License

See [LICENSE](LICENSE).

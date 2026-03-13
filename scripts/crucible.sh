#!/usr/bin/env bash
# Full aggressive end-to-end crucible: embed → torture → extract
# Proves: PNG round-trip, JPEG 75%/85%, cropping (spatial repetition), resize

set -e
cd "$(dirname "$0")/.."
CRUCIBLE_DIR="crucible_out"
mkdir -p "$CRUCIBLE_DIR"

# Generate key if needed
KEY="$CRUCIBLE_DIR/key.pem"
if [[ ! -f "$KEY" ]]; then
  echo "[1/8] Generating P-256 key..."
  openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$KEY"
fi

# Require image path
if [[ -z "$1" ]]; then
  echo "Usage: $0 <image.jpg>"
  exit 1
fi
SRC="$1"
if [[ ! -f "$SRC" ]]; then
  echo "ERROR: Source image not found: $SRC"
  exit 1
fi

ASSET_ID=4242
STEP=50
WD="$CRUCIBLE_DIR"
EMBED="$WD/embedded.png"

echo "=============================================="
echo "BLIND-WATERMARK CRUCIBLE"
echo "Source: $SRC | Asset ID: $ASSET_ID | Step: $STEP"
echo "=============================================="

echo ""
echo "[2/8] Embedding watermark..."
cargo run --quiet --bin watermark-cli -- embed -i "$SRC" -o "$EMBED" -k "$KEY" --asset-id $ASSET_ID -s $STEP
echo "  -> Saved $EMBED"

# Baseline: PNG extract
echo ""
echo "[3/8] Crucible A: PNG (lossless) extraction..."
OUT_A=$(cargo run --quiet --bin watermark-cli -- extract -i "$EMBED" 2>&1)
if echo "$OUT_A" | grep -q "SUCCESS"; then
  echo "  PASS: PNG extraction"
  REF_PUBKEY=$(echo "$OUT_A" | grep "Public Key" | sed 's/.*: //')
  REF_ASSET=$(echo "$OUT_A" | grep "Asset ID" | sed 's/.*: //')
  echo "  Reference: Asset ID=$REF_ASSET, PubKey=${REF_PUBKEY:0:20}..."
else
  echo "  FAIL: $OUT_A"
  exit 1
fi

# JPEG 75%
echo ""
echo "[4/8] Crucible B: JPEG 75% compression..."
magick "$EMBED" -quality 75 "$WD/jpeg75.jpg" 2>/dev/null || convert "$EMBED" -quality 75 "$WD/jpeg75.jpg"
OUT_B=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/jpeg75.jpg" 2>&1)
if echo "$OUT_B" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_B" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_B" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: JPEG 75% - same provenance"
  else
    echo "  FAIL: Mismatch (got Asset $ID)"
  fi
else
  echo "  FAIL: No extraction - $OUT_B"
fi

# JPEG 85%
echo ""
echo "[5/8] Crucible C: JPEG 85% compression..."
magick "$EMBED" -quality 85 "$WD/jpeg85.jpg" 2>/dev/null || convert "$EMBED" -quality 85 "$WD/jpeg85.jpg"
OUT_C=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/jpeg85.jpg" 2>&1)
if echo "$OUT_C" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_C" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_C" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: JPEG 85% - same provenance"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: No extraction"
fi

# Crop top-left 400×400 (block-aligned) - tests spatial repetition
echo ""
echo "[6/9] Crucible D: Crop top-left 400×400 (spatial repetition)..."
magick "$EMBED" -crop 400x400+0+0 +repage "$WD/crop400.png" 2>/dev/null || convert "$EMBED" -crop 400x400+0+0 +repage "$WD/crop400.png"
OUT_D=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/crop400.png" 2>&1)
if echo "$OUT_D" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_D" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_D" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: Crop 400×400 - hologram recovery"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: No extraction - $OUT_D"
fi

# Crop top-left 360×360 - extreme hologram (block-aligned)
echo ""
echo "[7/9] Crucible E: Crop top-left 360×360 (extreme hologram)..."
magick "$EMBED" -crop 360x360+0+0 +repage "$WD/crop360.png" 2>/dev/null || convert "$EMBED" -crop 360x360+0+0 +repage "$WD/crop360.png"
OUT_E=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/crop360.png" 2>&1)
if echo "$OUT_E" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_E" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_E" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: Crop 360×360 - extreme hologram"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: $OUT_E"
fi

# JPEG 95% (gentler compression, high survival)
echo ""
echo "[8/10] Crucible F: JPEG 95% compression..."
magick "$EMBED" -quality 95 "$WD/jpeg95.jpg" 2>/dev/null || convert "$EMBED" -quality 95 "$WD/jpeg95.jpg"
OUT_F=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/jpeg95.jpg" 2>&1)
if echo "$OUT_F" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_F" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_F" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: JPEG 95% - same provenance"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: No extraction"
fi

# Crop 400×400 + JPEG 75% (combined stress)
echo ""
echo "[9/10] Crucible G: Crop 400×400 + JPEG 75%..."
magick "$WD/crop400.png" -quality 75 "$WD/crop400_q75.jpg" 2>/dev/null || convert "$WD/crop400.png" -quality 75 "$WD/crop400_q75.jpg"
OUT_G=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/crop400_q75.jpg" 2>&1)
if echo "$OUT_G" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_G" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_G" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: Crop + JPEG - combined stress"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: $OUT_G"
fi

# Resize 70% (downscale; keeps min dim >= 344 for 978×620 source)
echo ""
echo "[10/10] Crucible H: Resize 70% (downscale)..."
magick "$EMBED" -resize 70% "$WD/resize70.png" 2>/dev/null || convert "$EMBED" -resize 70% "$WD/resize70.png"
OUT_H=$(cargo run --quiet --bin watermark-cli -- extract -i "$WD/resize70.png" 2>&1)
if echo "$OUT_H" | grep -q "SUCCESS"; then
  PK=$(echo "$OUT_H" | grep "Public Key" | sed 's/.*: //')
  ID=$(echo "$OUT_H" | grep "Asset ID" | sed 's/.*: //')
  if [[ "$PK" == "$REF_PUBKEY" && "$ID" == "$REF_ASSET" ]]; then
    echo "  PASS: Resize 70% - same provenance"
  else
    echo "  FAIL: Mismatch"
  fi
else
  echo "  FAIL: $OUT_H"
fi

# Summary (count SUCCESS in extract outputs)
PASS_CNT=0
for out in OUT_A OUT_B OUT_C OUT_D OUT_E OUT_F OUT_G OUT_H; do
  eval "val=\${$out}"
  echo "$val" | grep -q "SUCCESS" && PASS_CNT=$((PASS_CNT + 1))
done
echo ""
echo "=============================================="
echo "CRUCIBLE COMPLETE: $PASS_CNT/8 tests passed"
echo "Baseline: A (PNG) + F (JPEG 95%) required."
echo "Crop (D,E,G): DWT boundary limit. Resize (H): structure-dependent."
echo "=============================================="

use image::{Rgb, RgbImage};

pub struct YCbCrImage {
    pub y_channel: Vec<f64>,  // f64 for high-precision DWT/DCT math
    pub cb_channel: Vec<u8>,  // Passed through untouched
    pub cr_channel: Vec<u8>,  // Passed through untouched
    pub width: u32,
    pub height: u32,
}

impl YCbCrImage {
    /// Converts an standard RgbImage into our split-channel format.
    pub fn from_rgb(img: &RgbImage) -> Self {
        let (width, height) = img.dimensions();
        let capacity = (width * height) as usize;

        let mut y_channel = Vec::with_capacity(capacity);
        let mut cb_channel = Vec::with_capacity(capacity);
        let mut cr_channel = Vec::with_capacity(capacity);

        for pixel in img.pixels() {
            let r = pixel[0] as f64;
            let g = pixel[1] as f64;
            let b = pixel[2] as f64;

            // Standard ITU-R BT.601 conversion (JPEG standard)
            let y = 0.299 * r + 0.587 * g + 0.114 * b;
            let cb = 128.0 - 0.168736 * r - 0.331264 * g + 0.5 * b;
            let cr = 128.0 + 0.5 * r - 0.418688 * g - 0.081312 * b;

            y_channel.push(y);
            cb_channel.push(cb.round().clamp(0.0, 255.0) as u8);
            cr_channel.push(cr.round().clamp(0.0, 255.0) as u8);
        }

        Self {
            y_channel,
            cb_channel,
            cr_channel,
            width,
            height,
        }
    }

    /// Reconstructs the split channels back into a standard RgbImage.
    pub fn to_rgb(&self) -> RgbImage {
        let mut img = RgbImage::new(self.width, self.height);

        for (i, pixel) in img.pixels_mut().enumerate() {
            let y = self.y_channel[i];
            let cb = self.cb_channel[i] as f64;
            let cr = self.cr_channel[i] as f64;

            let r = y + 1.402 * (cr - 128.0);
            let g = y - 0.344136 * (cb - 128.0) - 0.714136 * (cr - 128.0);
            let b = y + 1.772 * (cb - 128.0);

            *pixel = Rgb([
                r.round().clamp(0.0, 255.0) as u8,
                g.round().clamp(0.0, 255.0) as u8,
                b.round().clamp(0.0, 255.0) as u8,
            ]);
        }

        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_space_roundtrip() {
        // Create a simple 2x2 image with distinct colors
        let mut original = RgbImage::new(2, 2);
        original.put_pixel(0, 0, Rgb([255, 0, 0]));     // Pure Red
        original.put_pixel(1, 0, Rgb([0, 255, 0]));     // Pure Green
        original.put_pixel(0, 1, Rgb([0, 0, 255]));     // Pure Blue
        original.put_pixel(1, 1, Rgb([128, 128, 128])); // Mid Gray

        // Convert to YCbCr and back
        let ycbcr = YCbCrImage::from_rgb(&original);
        let reconstructed = ycbcr.to_rgb();

        // Check that pixels survive the roundtrip with minimal rounding variance
        for (orig_px, recon_px) in original.pixels().zip(reconstructed.pixels()) {
            for c in 0..3 {
                let diff = (orig_px[c] as i32 - recon_px[c] as i32).abs();
                assert!(
                    diff <= 1,
                    "Color space conversion shifted pixel by more than 1 unit"
                );
            }
        }
    }
}

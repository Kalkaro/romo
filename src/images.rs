use crate::geometry::Rect;
use base64::{Engine, engine::general_purpose::STANDARD};
use gpui::RenderImage;
use image::ImageReader;
use serde::{Deserialize, Deserializer, Serialize};
use std::{io::Cursor, sync::Arc};

/// Portable PNG data shared by the board, clipboard and undo history.
#[derive(Clone, Serialize)]
pub struct ImageAsset {
    pub png: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub rendered: Arc<RenderImage>,
}
impl std::fmt::Debug for ImageAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageAsset")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}
impl PartialEq for ImageAsset {
    fn eq(&self, other: &Self) -> bool {
        self.png == other.png
    }
}
impl<'de> Deserialize<'de> for ImageAsset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            png: String,
            width: u32,
            height: u32,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        if encoded.png.len() > 12 * 1024 * 1024 {
            return Err(serde::de::Error::custom("Image exceeds 9 MB"));
        }
        let bytes = STANDARD
            .decode(&encoded.png)
            .map_err(serde::de::Error::custom)?;
        let mut asset = Self::from_bytes(&bytes).map_err(serde::de::Error::custom)?;
        if (asset.width, asset.height) != (encoded.width, encoded.height) {
            return Err(serde::de::Error::custom(
                "Image dimensions do not match its data",
            ));
        }
        asset.png = encoded.png;
        Ok(asset)
    }
}
impl ImageAsset {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 9 * 1024 * 1024 {
            return Err("Images must be smaller than 9 MB.".into());
        }
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|e| format!("Cannot read image: {e}"))?;
        if u64::from(decoded.width()) * u64::from(decoded.height()) > 16_000_000 {
            return Err("Images must contain fewer than 16 million pixels.".into());
        }
        let mut png = Cursor::new(Vec::new());
        decoded
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let png = png.into_inner();
        if png.len() > 9 * 1024 * 1024 {
            return Err("The decoded image is too large to store in a board.".into());
        }
        let (width, height) = (decoded.width(), decoded.height());
        let mut rgba = decoded.into_rgba8();
        // GPUI's sprite atlas expects BGRA pixels.
        for pixel in rgba.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Self {
            png: STANDARD.encode(png),
            width,
            height,
            rendered: Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])),
        })
    }
    pub fn bytes(&self) -> Vec<u8> {
        STANDARD.decode(&self.png).expect("Validated PNG")
    }
    /// The whole pixels that `crop` covers, as x, y, width and height, or the
    /// whole image without a crop.
    fn region(&self, crop: Option<Rect>) -> (usize, usize, usize, usize) {
        let (width, height) = (self.width as f32, self.height as f32);
        let Some(crop) = crop else {
            return (0, 0, self.width as usize, self.height as usize);
        };
        let x = crop.min.x.round().max(0.0).min(width - 1.0);
        let y = crop.min.y.round().max(0.0).min(height - 1.0);
        let right = crop.max.x.round().max(x + 1.0).min(width);
        let bottom = crop.max.y.round().max(y + 1.0).min(height);
        (
            x as usize,
            y as usize,
            (right - x) as usize,
            (bottom - y) as usize,
        )
    }
    /// RGBA pixels of the part of the image within `crop`, or of all of it.
    pub fn pixels(&self, crop: Option<Rect>) -> image::RgbaImage {
        let (x, y, width, height) = self.region(crop);
        let source = self.rendered.as_bytes(0).unwrap_or_default();
        let mut pixels = image::RgbaImage::new(width as u32, height as u32);
        for (row, line) in pixels.chunks_exact_mut((width * 4).max(1)).enumerate() {
            let start = ((y + row) * self.width as usize + x) * 4;
            line.copy_from_slice(&source[start..start + line.len()]);
        }
        // The rendered copy is in BGRA order.
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        pixels
    }
    /// PNG data for the part of the image within `crop`, or for all of it.
    pub fn png(&self, crop: Option<Rect>) -> Result<Vec<u8>, String> {
        if crop.is_none() {
            return Ok(self.bytes());
        }
        let mut png = Cursor::new(Vec::new());
        self.pixels(crop)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(png.into_inner())
    }
    /// Whether `other` holds exactly this image's pixels within `crop`.
    pub fn matches(&self, crop: Option<Rect>, other: &Self) -> bool {
        let (x, y, width, height) = self.region(crop);
        let (Some(mine), Some(theirs)) = (self.rendered.as_bytes(0), other.rendered.as_bytes(0))
        else {
            return false;
        };
        (other.width as usize, other.height as usize) == (width, height)
            && theirs
                .chunks_exact((width * 4).max(1))
                .enumerate()
                .all(|(row, line)| {
                    let start = ((y + row) * self.width as usize + x) * 4;
                    mine[start..start + line.len()] == *line
                })
    }
    /// A small gradient whose red and green channels count columns and rows.
    #[cfg(test)]
    pub fn sample(width: u32, height: u32) -> Self {
        let source = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 150, 255])
        });
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        Self::from_bytes(&bytes.into_inner()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_survives_portable_board_round_trip_with_color_and_alpha() {
        let source = image::RgbaImage::from_pixel(12, 8, image::Rgba([210, 90, 35, 180]));
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let asset = ImageAsset::from_bytes(&bytes.into_inner()).unwrap();
        let mut board = crate::model::Board::default();
        let id = board.add(
            crate::model::Kind::Image,
            crate::geometry::Pos2::ZERO,
            crate::model::PALETTE[7],
        );
        board.get_mut(id).unwrap().image = Some(Arc::new(asset));
        let restored: crate::model::Board =
            serde_json::from_str(&serde_json::to_string(&board).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, board);
        let asset = restored.items[0].image.as_ref().unwrap();
        let pixel = image::load_from_memory(&asset.bytes())
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0;
        assert_eq!(pixel, [210, 90, 35, 180]);
        assert!(crate::svg::export(&restored).contains("data:image/png;base64,"));
        assert!(ImageAsset::from_bytes(b"not an image").is_err());
    }

    #[test]
    fn a_cropped_copy_holds_only_the_pixels_that_show() {
        use crate::geometry::pos2;
        let asset = ImageAsset::sample(40, 30);
        let crop = Rect::from_min_max(pos2(10.0, 5.0), pos2(30.0, 25.0));
        let pixels = asset.pixels(Some(crop));
        assert_eq!(pixels.dimensions(), (20, 20));
        assert_eq!(pixels.get_pixel(0, 0).0, [10, 5, 150, 255]);
        assert_eq!(pixels.get_pixel(19, 19).0, [29, 24, 150, 255]);
        let copied = ImageAsset::from_bytes(&asset.png(Some(crop)).unwrap()).unwrap();
        assert!(asset.matches(Some(crop), &copied));
        assert!(!asset.matches(None, &copied));
        assert!(asset.matches(None, &asset));
        assert_eq!(asset.pixels(None).get_pixel(39, 29).0, [39, 29, 150, 255]);
    }
}

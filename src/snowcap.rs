use std::ops::Range;

#[derive(Clone, Copy, PartialEq)]
pub struct SnowCap {
    pub width: i32,
    pub depth: f32,
    pub roughness: f32,
    pub seed: u32,
    pub icicles: bool,
}

impl SnowCap {
    pub fn rows(&self) -> Range<i32> {
        -(self.depth.ceil() as i32)
            ..(self.depth * 0.43 + 3.5).ceil() as i32 + if self.icicles { 14 } else { 0 }
    }

    /// Coordinates are relative to the window's upper left corner. Pixels are
    /// premultiplied ARGB, as required by the layered Win32 window.
    pub fn rasterize(
        &self,
        columns: Range<i32>,
        rows: Range<i32>,
        put: impl FnMut(i32, i32, u32),
    ) {
        self.rasterize_removed(columns, rows, &[], 10, put);
    }

    pub fn sample_height(&self, x: f32) -> f32 {
        self.crest(x)
    }

    /// A window can lose snow locally to rolling snowballs. The removed
    /// heights are stored at fixed-width segments of the upper edge.
    pub fn rasterize_removed(
        &self,
        columns: Range<i32>,
        rows: Range<i32>,
        removed: &[f32],
        segment_width: usize,
        mut put: impl FnMut(i32, i32, u32),
    ) {
        let span = self.width as f32;
        let local_height = |x: f32| {
            let segment = (x.max(0.0) as usize / segment_width.max(1)).min(removed.len().saturating_sub(1));
            (self.crest(x) - removed.get(segment).copied().unwrap_or(0.0)).max(0.0)
        };
        for x in columns.start.max(0)..columns.end.min(self.width) {
            let center = x as f32 + 0.5;
            let t = center / span;
            let crest = local_height(center);
            if crest < 0.16 {
                continue;
            }

            let slope = (local_height(center + 1.5) - local_height(center - 1.5)) / 3.0;
            let drift = noise(self.seed ^ 0x6fd5_8e2b, t * (3.0 + span / 90.0));
            let scallop = noise(self.seed ^ 0x892a_37df, center / 19.0);
            let lip = crest * (0.13 + 0.22 * drift + 0.08 * scallop);
            let shadow_width = (crest * 0.10 + 1.0).min(3.5);
            let thickness = crest + lip;
            let (ice_length, ice_center) = self.icicle(center, lip);
            let bottom = lip + shadow_width.max(ice_length);
            let start = rows.start.max((-crest).floor() as i32);
            let end = rows.end.min(bottom.ceil() as i32);
            let broad_light = 5.0 * (noise(self.seed ^ 0x7f61_123c, center / 11.0) - 0.5);

            for y in start..end {
                let center_y = y as f32 + 0.5;
                let coverage = (center_y + crest + 0.5).clamp(0.0, 1.0)
                    * (lip - center_y + 0.5).clamp(0.0, 1.0);
                let pixel = if coverage > 0.0 {
                    let volume = ((center_y + crest) / thickness).clamp(0.0, 1.0);
                    let shade = 28.0 * smooth((volume - 0.20) / 0.80)
                        + 16.0 * smooth((volume - 0.73) / 0.27);
                    let side_light = slope.clamp(-1.0, 1.0) * -9.0 * (1.0 - volume);
                    let grain = unit(
                        self.seed
                            ^ (x as u32).wrapping_mul(0x9e37_79b9)
                            ^ (y as u32).wrapping_mul(0x85eb_ca77),
                    );
                    let brightness = broad_light + side_light + (grain - 0.5) * 7.0 - shade;
                    argb(
                        [
                            248.0 + brightness,
                            251.0 + brightness * 0.73,
                            253.0 + brightness * 0.44,
                        ],
                        coverage * 250.0,
                    )
                } else if ice_length > 0.0 && center_y < lip + ice_length {
                    let taper = 1.0 - ((center_y - lip) / ice_length).clamp(0.0, 1.0);
                    let alpha = (ice_center * taper + 0.25).clamp(0.0, 1.0);
                    argb([178.0, 214.0, 234.0], 180.0 * alpha)
                } else {
                    let falloff = (1.0 - (center_y - lip) / shadow_width).clamp(0.0, 1.0);
                    argb(
                        [21.0, 38.0, 54.0],
                        58.0 * falloff * falloff * (crest / 4.0).min(1.0),
                    )
                };
                if pixel != 0 {
                    put(x, y, pixel);
                }
            }
        }
    }

    fn crest(&self, x: f32) -> f32 {
        let span = self.width as f32;
        let t = x / span;
        let roughness = self.roughness.clamp(0.0, 1.0);
        let rim = (self.depth * 1.45 + 5.0).min(span * 0.24);
        let left = rim * (0.8 + 0.4 * unit(self.seed ^ 0x6b59_ce03));
        let right = rim * (0.8 + 0.4 * unit(self.seed ^ 0xcbd9_15cf));
        let rounded_ends = smooth((x / left).min((span - x) / right)).sqrt();
        let banks = noise(self.seed, t * (2.4 + span / 225.0));
        let lumps = noise(self.seed ^ 0x1ecb_431d, t * (4.0 + span / 58.0));
        let powder = noise(self.seed ^ 0x38a9_c9e1, x / 7.5);
        let profile = 0.62
            + (banks - 0.5) * (0.52 + 0.38 * roughness)
            + (lumps - 0.5) * (0.17 + 0.24 * roughness)
            + (powder - 0.5) * (0.04 + 0.10 * roughness);
        // An initial dusting takes hold on the thicker parts of the profile first.
        let dusting = (1.0 - self.depth / 4.5).max(0.0) * 0.31;
        self.depth * rounded_ends * (profile - dusting).clamp(0.0, 1.0)
    }

    fn icicle(&self, x: f32, lip: f32) -> (f32, f32) {
        if !self.icicles || self.depth < 5.0 || lip < 1.3 {
            return (0.0, 0.0);
        }
        let cell = (x / 52.0).floor() as u32;
        let sample = self.seed ^ cell.wrapping_mul(0xb529_7a4d);
        let center = 7.0 + 38.0 * unit(sample);
        let half_width = 0.9 + 0.7 * unit(sample ^ 0x047a_ad81);
        let weight = (1.0 - (x % 52.0 - center).abs() / half_width).max(0.0);
        let length = (self.depth * (0.24 + 0.48 * unit(sample ^ 0x852a_f1ed))).min(13.0);
        (length * weight, weight)
    }
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn noise(seed: u32, x: f32) -> f32 {
    let cell = x.floor();
    let left = seed ^ (cell as i32 as u32).wrapping_mul(0x9e37_79b9);
    let right = seed ^ ((cell as i32 + 1) as u32).wrapping_mul(0x9e37_79b9);
    let first = unit(left);
    first + (unit(right) - first) * smooth(x - cell)
}

fn unit(mut value: u32) -> f32 {
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846c_a68b);
    ((value ^ (value >> 16)) >> 8) as f32 / 16_777_216.0
}

fn argb(rgb: [f32; 3], alpha: f32) -> u32 {
    let alpha = alpha.round() as u32;
    let channel = |v: f32| v.clamp(0.0, 255.0).round() as u32 * alpha / 255;
    (alpha << 24) | (channel(rgb[0]) << 16) | (channel(rgb[1]) << 8) | channel(rgb[2])
}

#[cfg(test)]
mod tests {
    use super::SnowCap;

    #[test]
    fn rolling_removes_snow_only_from_touched_segments() {
        let cap = SnowCap {
            width: 300,
            depth: 20.0,
            roughness: 0.35,
            seed: 1024,
            icicles: false,
        };
        let mut untouched = 0usize;
        let mut original = 0usize;
        cap.rasterize(0..cap.width, cap.rows(), |x, _, pixel| {
            if (120..150).contains(&x) && pixel >> 24 > 128 { original += 1; }
            if (200..240).contains(&x) && pixel >> 24 > 128 { untouched += 1; }
        });
        let mut carved = vec![0.0; 30];
        for segment in &mut carved[12..15] { *segment = 15.0; }
        let mut removed_region = 0usize;
        let mut unmodified_region = 0usize;
        cap.rasterize_removed(0..cap.width, cap.rows(), &carved, 10, |x, _, pixel| {
            if (120..150).contains(&x) && pixel >> 24 > 128 { removed_region += 1; }
            if (200..240).contains(&x) && pixel >> 24 > 128 { unmodified_region += 1; }
        });
        assert!(removed_region < original);
        assert_eq!(unmodified_region, untouched);
    }

    #[test]
    fn clipping_preserves_the_profile_and_premultiplied_channels() {
        for depth in [1.0, 18.0, 50.0] {
            let cap = SnowCap {
                width: 670,
                depth,
                roughness: 0.45,
                seed: 42,
                icicles: true,
            };
            let mut expected = Vec::new();
            cap.rasterize(0..cap.width, cap.rows(), |x, y, pixel| {
                let alpha = pixel >> 24;
                for shift in [0, 8, 16] {
                    assert!((pixel >> shift) & 255 <= alpha);
                }
                if x >= 350 && y >= -8 && y < 9 {
                    expected.push((x, y, pixel));
                }
            });
            let mut actual = Vec::new();
            cap.rasterize(350..800, -8..9, |x, y, pixel| actual.push((x, y, pixel)));
            assert!(!expected.is_empty());
            assert_eq!(actual, expected);
        }
    }
}

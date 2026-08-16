use image::RgbaImage;

const CANVAS_RESIZE_HEADROOM: u32 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchFrameStatus {
    Appended,
    Stationary,
    LowConfidence,
    Reverse,
}

#[derive(Debug, Clone)]
pub struct StitchFrameResult {
    pub status: StitchFrameStatus,
    pub height: i32,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StitchConfig {
    pub min_overlap: u32,
    pub min_scroll_threshold: u32,
    pub dynamic_block_size: usize,
    pub dynamic_threshold: f32,
    pub min_content_blocks: usize,
    pub content_energy_ratio: f32,
    pub max_reverse_scroll: i32,
    /// Largest forward scroll (in px) to search for between two frames. A
    /// momentum flick can exceed this; such frames are simply skipped until the
    /// scroll slows enough that consecutive frames overlap within this range.
    pub max_scroll_per_frame: i32,
    /// Column/row stride for the coarse ZNCC scan (>=1). The full-overlap ZNCC
    /// metric is decisive enough to survive subsampling, which keeps the scan
    /// cheap enough to run every frame.
    pub scan_stride: usize,
    pub low_confidence_threshold: f32,
    pub low_confidence_gap: f32,
    pub seam_margin_divisor: u32,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            min_overlap: 24,
            min_scroll_threshold: 4,
            dynamic_block_size: 16,
            dynamic_threshold: 12.0,
            min_content_blocks: 3,
            content_energy_ratio: 0.12,
            max_reverse_scroll: 8,
            max_scroll_per_frame: 400,
            scan_stride: 4,
            low_confidence_threshold: 0.6,
            low_confidence_gap: 0.0,
            seam_margin_divisor: 5,
        }
    }
}

#[derive(Default)]
struct FrameAnalysis {
    width: usize,
    height: usize,
    gray: Vec<f32>,
    edge: Vec<f32>,
}

impl FrameAnalysis {
    fn from_image(image: &RgbaImage) -> Self {
        let width = image.width() as usize;
        let height = image.height() as usize;
        let gray = ScrollStitcher::grayscale(image);
        let edge = ScrollStitcher::edge_energy(&gray, width, height);
        Self { width, height, gray, edge }
    }
}

#[derive(Default)]
struct StitchScratch {
    content_blocks: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct StitchRegion {
    width: usize,
    height: usize,
    fixed_top: usize,
    fixed_bottom: usize,
}

impl StitchRegion {
    fn valid_height(self) -> usize {
        self.height.saturating_sub(self.fixed_top + self.fixed_bottom)
    }
}

#[derive(Clone, Copy)]
struct FramePairRef<'a> {
    prev: &'a [f32],
    next: &'a [f32],
}

#[derive(Debug, Clone, Copy)]
struct StitchAppendPlan {
    trim_amount: u32,
    append_start_y: u32,
    append_end_y: u32,
    fixed_bottom: u32,
}

#[derive(Default)]
struct ThumbnailCache {
    target_width: u32,
    source_width: u32,
    source_height: u32,
    dirty_from: u32,
    image: Option<RgbaImage>,
}

impl ThumbnailCache {
    fn reset(&mut self) {
        self.target_width = 0;
        self.source_width = 0;
        self.source_height = 0;
        self.dirty_from = 0;
        self.image = None;
    }

    fn mark_dirty_from(&mut self, source_row: u32) {
        if self.image.is_none() {
            return;
        }
        self.dirty_from = self.dirty_from.min(source_row);
    }
}

pub struct ScrollStitcher {
    canvas: Option<RgbaImage>,
    valid_height: u32,
    last_analysis: Option<FrameAnalysis>,
    last_footer_height: u32,
    config: StitchConfig,
    scratch: StitchScratch,
    thumbnail: ThumbnailCache,
}

impl Default for ScrollStitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollStitcher {
    pub fn new() -> Self {
        Self::with_config(StitchConfig::default())
    }

    pub fn with_config(config: StitchConfig) -> Self {
        Self {
            canvas: None,
            valid_height: 0,
            last_analysis: None,
            last_footer_height: 0,
            config,
            scratch: StitchScratch::default(),
            thumbnail: ThumbnailCache::default(),
        }
    }

    pub fn current_image(&self) -> Option<(&RgbaImage, u32)> {
        self.canvas.as_ref().map(|img| (img, self.valid_height))
    }

    pub fn get_final_image(&self) -> Option<RgbaImage> {
        let (canvas, h) = self.current_image()?;
        if h == 0 || canvas.width() == 0 {
            return None;
        }
        let mut final_img = RgbaImage::new(canvas.width(), h);
        Self::copy_region(canvas, 0, &mut final_img, 0, h);
        Some(final_img)
    }

    pub fn make_thumbnail(&mut self, target_width: u32) -> Option<RgbaImage> {
        let canvas = self.canvas.as_ref()?;
        let valid_h = self.valid_height;
        let source_width = canvas.width();
        if target_width == 0 || valid_h == 0 || source_width == 0 {
            return None;
        }

        let scale = target_width as f32 / source_width as f32;
        let target_height = (valid_h as f32 * scale) as u32;
        if target_height == 0 {
            return None;
        }

        let requires_full_render = self.thumbnail.image.is_none()
            || self.thumbnail.target_width != target_width
            || self.thumbnail.source_width != source_width
            || valid_h < self.thumbnail.source_height;

        if requires_full_render {
            let mut cropped = RgbaImage::new(source_width, valid_h);
            Self::copy_region(canvas, 0, &mut cropped, 0, valid_h);
            self.thumbnail.image = Some(image::imageops::resize(
                &cropped,
                target_width,
                target_height,
                image::imageops::FilterType::Triangle,
            ));
            self.thumbnail.target_width = target_width;
            self.thumbnail.source_width = source_width;
            self.thumbnail.source_height = valid_h;
            self.thumbnail.dirty_from = valid_h;
            return self.thumbnail.image.clone();
        }

        let dirty_from = self.thumbnail.dirty_from.min(valid_h);
        if dirty_from >= valid_h && self.thumbnail.source_height == valid_h {
            return self.thumbnail.image.clone();
        }

        let previous_thumbnail = self.thumbnail.image.as_ref()?;
        let mut next_thumbnail = if previous_thumbnail.height() == target_height {
            previous_thumbnail.clone()
        } else {
            let mut resized = RgbaImage::new(target_width, target_height);
            let preserved_rows = ((dirty_from as f32) * scale).floor() as u32;
            let copy_rows = preserved_rows.min(previous_thumbnail.height()).min(target_height);
            if copy_rows > 0 {
                Self::copy_region(previous_thumbnail, 0, &mut resized, 0, copy_rows);
            }
            resized
        };

        let source_start = if valid_h > self.thumbnail.source_height {
            dirty_from.saturating_sub(1)
        } else {
            dirty_from
        };
        let target_start = ((source_start as f32) * scale).floor() as u32;
        let source_h = valid_h.saturating_sub(source_start);
        let target_h = target_height.saturating_sub(target_start);

        if source_h > 0 && target_h > 0 {
            let strip = image::imageops::crop_imm(canvas, 0, source_start, source_width, source_h).to_image();
            let resized_strip = image::imageops::resize(&strip, target_width, target_h, image::imageops::FilterType::Triangle);
            Self::copy_region(&resized_strip, 0, &mut next_thumbnail, target_start, target_h);
        }

        self.thumbnail.image = Some(next_thumbnail);
        self.thumbnail.target_width = target_width;
        self.thumbnail.source_width = source_width;
        self.thumbnail.source_height = valid_h;
        self.thumbnail.dirty_from = valid_h;
        self.thumbnail.image.clone()
    }

    pub fn process_frame_detailed(&mut self, new_image: RgbaImage) -> StitchFrameResult {
        if self.canvas.is_none() {
            self.initialize_canvas(new_image);
            return StitchFrameResult {
                status: StitchFrameStatus::Appended,
                height: self.valid_height as i32,
                warning: None,
            };
        }

        let Some(prev_analysis) = self.last_analysis.take() else {
            self.initialize_canvas(new_image);
            return StitchFrameResult {
                status: StitchFrameStatus::Appended,
                height: self.valid_height as i32,
                warning: None,
            };
        };

        let next_analysis = FrameAnalysis::from_image(&new_image);
        if prev_analysis.width != next_analysis.width || prev_analysis.height != next_analysis.height {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = 0;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Frame geometry changed while scrolling".to_string()),
            };
        }

        let width = prev_analysis.width;
        let height = prev_analysis.height;
        let (fixed_top, fixed_bottom) = self.detect_sticky_regions(&prev_analysis.gray, &next_analysis.gray, &prev_analysis.edge, width, height);
        let region = StitchRegion {
            width,
            height,
            fixed_top: fixed_top as usize,
            fixed_bottom: fixed_bottom as usize,
        };
        let valid_h = (height as u32).saturating_sub(fixed_top + fixed_bottom);
        if valid_h < self.config.min_overlap {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Insufficient scrollable content in selection".to_string()),
            };
        }

        let gray_pair = FramePairRef {
            prev: &prev_analysis.gray,
            next: &next_analysis.gray,
        };
        Self::detect_content_blocks(self.config, &prev_analysis.edge, &next_analysis.edge, region, &mut self.scratch.content_blocks);

        if self.scratch.content_blocks.len() < self.config.min_content_blocks {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Not enough textured content to track scrolling".to_string()),
            };
        }

        // Locate the scroll offset by scanning shifts with the full-overlap
        // ZNCC metric directly (see refine_shift_zncc). No coarse row-signature
        // seed: on self-similar content that proxy returned wrong seeds and the
        // true offset fell outside the refine window.
        let refine = Self::refine_shift_zncc(self.config, gray_pair, region, &self.scratch.content_blocks);

        let Some((delta, best_score, second_score)) = refine else {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Unable to estimate reliable overlap".to_string()),
            };
        };

        // Trust the estimated shift BEFORE interpreting its sign or magnitude.
        // A momentum flick can move 500+px between two 16ms frames, collapsing
        // the overlap so the matcher locks onto a garbage peak at the search
        // boundary (near-zero score, often negative). Classifying that as
        // "reverse" or appending it would corrupt the stitch, so a low-
        // confidence frame is simply skipped — and we KEEP the previous
        // reference frame so a single bad match does not discard the scroll
        // position accumulated so far.
        let confidence_gap = best_score - second_score;
        if best_score < self.config.low_confidence_threshold || confidence_gap < self.config.low_confidence_gap {
            self.last_analysis = Some(prev_analysis);
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Low confidence overlap match; keep scrolling smoothly".to_string()),
            };
        }

        if delta < 0 {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::Reverse,
                height: self.valid_height as i32,
                warning: Some("Detected reverse scrolling; capture paused".to_string()),
            };
        }

        let delta = delta as u32;
        if delta < self.config.min_scroll_threshold {
            // Sub-threshold motion: keep the PREVIOUS reference frame instead of
            // advancing to this one. During slow scrolling each ~16ms frame
            // moves only a pixel or two — if we reset the reference every time,
            // that motion never accumulates and nothing is ever appended (the
            // canvas freezes at the first frame). Holding the reference lets the
            // offset grow across frames until it crosses the threshold.
            self.last_analysis = Some(prev_analysis);
            return StitchFrameResult {
                status: StitchFrameStatus::Stationary,
                height: self.valid_height as i32,
                warning: None,
            };
        }

        let valid_h_usize = valid_h as usize;
        let delta_usize = delta as usize;
        if delta_usize >= valid_h_usize {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Overlap collapsed due to unstable motion".to_string()),
            };
        }

        let overlap_valid = valid_h_usize.saturating_sub(delta_usize);
        if overlap_valid < self.config.min_overlap as usize {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Overlap too small; scroll slower".to_string()),
            };
        }

        let cut_valid = self.find_smart_seam(gray_pair, region, overlap_valid, &self.scratch.content_blocks);

        let trim_prev = overlap_valid.saturating_sub(cut_valid);
        let append_start = fixed_top as usize + cut_valid;
        let append_end = (new_image.height().saturating_sub(fixed_bottom)) as usize;

        if append_end <= append_start {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            return StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("No appendable content after sticky region filtering".to_string()),
            };
        }

        let plan = StitchAppendPlan {
            trim_amount: trim_prev as u32,
            append_start_y: append_start as u32,
            append_end_y: append_end as u32,
            fixed_bottom,
        };

        if self.execute_stitch(&new_image, plan) {
            self.last_analysis = Some(next_analysis);
            StitchFrameResult {
                status: StitchFrameStatus::Appended,
                height: self.valid_height as i32,
                warning: None,
            }
        } else {
            self.last_analysis = Some(next_analysis);
            self.last_footer_height = fixed_bottom;
            StitchFrameResult {
                status: StitchFrameStatus::LowConfidence,
                height: self.valid_height as i32,
                warning: Some("Failed to append frame into scroll canvas".to_string()),
            }
        }
    }

    fn initialize_canvas(&mut self, first_image: RgbaImage) {
        let w = first_image.width();
        let h = first_image.height();
        let mut canvas = RgbaImage::new(w, h * 3);
        Self::copy_region(&first_image, 0, &mut canvas, 0, h);

        self.canvas = Some(canvas);
        self.valid_height = h;
        self.last_analysis = Some(FrameAnalysis::from_image(&first_image));
        self.last_footer_height = 0;
        self.thumbnail.reset();
    }

    fn grayscale(image: &RgbaImage) -> Vec<f32> {
        image
            .as_raw()
            .chunks_exact(4)
            .map(|px| (0.299 * f32::from(px[0])) + (0.587 * f32::from(px[1])) + (0.114 * f32::from(px[2])))
            .collect()
    }

    fn edge_energy(gray: &[f32], width: usize, height: usize) -> Vec<f32> {
        let mut out = vec![0.0; gray.len()];
        if width < 3 || height < 3 {
            return out;
        }

        for y in 1..(height - 1) {
            let row = y * width;
            for x in 1..(width - 1) {
                let i = row + x;
                let gx = gray[i + 1] - gray[i - 1];
                let gy = gray[i + width] - gray[i - width];
                out[i] = gx.abs() + gy.abs();
            }
        }
        out
    }

    /// Detect fixed (sticky) header/footer bands that stay put while the body
    /// scrolls, so they are not appended repeatedly.
    ///
    /// A band only counts as sticky when it is both *unchanged* between frames
    /// AND carries real content (edge energy). Blank rows are unchanged too —
    /// the whitespace at the top/bottom of a scrolling viewport looks identical
    /// frame to frame — but they are empty scroll space, not a sticky UI band.
    /// Treating them as sticky would trim real content out of the stitch.
    fn detect_sticky_regions(&self, prev: &[f32], next: &[f32], edge_prev: &[f32], width: usize, height: usize) -> (u32, u32) {
        if width == 0 || height == 0 {
            return (0, 0);
        }

        let diff_threshold = self.config.dynamic_threshold;
        let content_threshold = self.config.dynamic_threshold;
        let max_check = (height / 3).max(1);

        let row_metrics = |row: usize| -> (f32, f32) {
            let row_start = row * width;
            let mut diff = 0.0f32;
            let mut energy = 0.0f32;
            for x in 0..width {
                let idx = row_start + x;
                diff += (prev[idx] - next[idx]).abs();
                energy += edge_prev[idx];
            }
            (diff / width as f32, energy / width as f32)
        };

        let mut top = 0usize;
        while top < max_check {
            let (diff, energy) = row_metrics(top);
            // Stop at the first row that either moved or is blank: a sticky band
            // is a contiguous run of unchanged, content-bearing rows.
            if diff > diff_threshold || energy < content_threshold {
                break;
            }
            top += 1;
        }

        let mut bottom = 0usize;
        while bottom < max_check {
            let (diff, energy) = row_metrics(height - 1 - bottom);
            if diff > diff_threshold || energy < content_threshold {
                break;
            }
            bottom += 1;
        }

        (top as u32, bottom as u32)
    }

    /// Select the vertical column-blocks that carry enough texture (edge
    /// energy) to be useful for vertical scroll correlation.
    ///
    /// Scroll stitching aligns two frames by matching their textured content.
    /// Flat regions — whitespace margins, solid backgrounds — carry no vertical
    /// signal and must be skipped. We therefore keep the blocks whose combined
    /// edge energy is a meaningful fraction of the busiest block, rather than
    /// the blocks that happen to be *unchanged* between frames (which, while
    /// scrolling, are exactly the empty margins and thus useless for matching).
    fn detect_content_blocks(config: StitchConfig, edge_prev: &[f32], edge_next: &[f32], region: StitchRegion, out: &mut Vec<usize>) {
        let block = config.dynamic_block_size.max(8);
        let block_count = (region.width / block).max(1);
        let y_start = region.fixed_top.min(region.height);
        let y_end = region.height.saturating_sub(region.fixed_bottom).max(y_start + 1);

        out.clear();
        if out.capacity() < block_count {
            out.reserve(block_count - out.capacity());
        }

        let mut energies = Vec::with_capacity(block_count);
        let mut max_energy = 0.0f32;
        for b in 0..block_count {
            let x0 = b * block;
            let x1 = ((b + 1) * block).min(region.width);
            if x1 <= x0 {
                energies.push(0.0);
                continue;
            }

            let mut energy_sum = 0.0f32;
            let mut count = 0usize;
            for y in y_start..y_end {
                let row = y * region.width;
                for x in x0..x1 {
                    let i = row + x;
                    energy_sum += edge_prev[i] + edge_next[i];
                    count += 1;
                }
            }

            let energy = if count == 0 { 0.0 } else { energy_sum / count as f32 };
            max_energy = max_energy.max(energy);
            energies.push(energy);
        }

        if max_energy <= f32::EPSILON {
            return;
        }

        let threshold = max_energy * config.content_energy_ratio;
        for (b, &energy) in energies.iter().enumerate() {
            if energy >= threshold {
                out.push(b);
            }
        }
    }

    /// Find the vertical scroll offset between two frames by directly scanning
    /// candidate shifts with the full-overlap ZNCC metric.
    ///
    /// The earlier design seeded this from a multi-scale row-signature search,
    /// but that proxy returned wildly wrong (often negative) seeds on
    /// self-similar content — text pages, repeated list rows — so the true
    /// offset fell outside the narrow refine window and every frame was
    /// rejected. Because the full-overlap ZNCC is decisive (a correct shift
    /// scores ~1.0, a wrong one ~0.0), we can afford to scan the whole
    /// plausible range instead: a strided coarse pass to locate the peak, then
    /// a 1px fine pass around it. `coarse_shift` from the old search is ignored.
    fn refine_shift_zncc(
        config: StitchConfig,
        pair: FramePairRef<'_>,
        region: StitchRegion,
        content_blocks: &[usize],
    ) -> Option<(i32, f32, f32)> {
        let valid_h = region.valid_height() as i32;
        if valid_h <= config.min_overlap as i32 {
            return None;
        }

        let max_shift = valid_h - config.min_overlap as i32;
        if max_shift <= 0 {
            return None;
        }

        // Search forward scrolls up to max_scroll_per_frame, plus a small
        // reverse margin so genuine slow reverse scrolling is still detected.
        let forward_limit = config.max_scroll_per_frame.min(max_shift);
        let reverse_limit = config.max_reverse_scroll.min(max_shift);
        let stride = config.scan_stride.max(1) as i32;

        // Periodic content (uniform line pitch, repeated rows) produces aliased
        // peaks: a shifted frame scores nearly as high at shift = k*line-period
        // as at the true offset. Among all shifts scoring within `tie_margin` of
        // the best, we prefer the smallest magnitude — the true scroll between
        // two 16ms frames is small, while aliases sit a full line-period away.
        // Genuine scrolling is unaffected: the true shift is highest AND
        // smallest. Two passes over a strided grid: find the global max, then
        // pick the nearest-to-zero shift that comes within tie_margin of it.
        let tie_margin = 0.02f32;
        let stride_us = stride.max(1) as usize;

        // Build the strided candidate grid (always including 0).
        let mut candidates: Vec<i32> = Vec::new();
        candidates.push(0);
        let mut s = stride;
        while s <= forward_limit {
            candidates.push(s);
            s += stride;
        }
        let mut s = -stride;
        while s >= -reverse_limit {
            candidates.push(s);
            s -= stride;
        }

        let mut global_best = f32::MIN;
        let mut scored: Vec<(i32, f32)> = Vec::with_capacity(candidates.len());
        for &shift in &candidates {
            let score = Self::zncc_score_for_shift(config, pair, region, shift, content_blocks, stride_us);
            if score > global_best {
                global_best = score;
            }
            scored.push((shift, score));
        }

        let mut coarse_best_shift = 0i32;
        let mut coarse_pick_mag = i32::MAX;
        for &(shift, score) in &scored {
            if score >= global_best - tie_margin && shift.abs() < coarse_pick_mag {
                coarse_pick_mag = shift.abs();
                coarse_best_shift = shift;
            }
        }

        // Fine 1px pass around the coarse peak, scored at full resolution. The
        // coarse pass already resolved the period-aliasing ambiguity by picking
        // the smallest-magnitude peak, so here we want the single highest-
        // scoring shift for a pixel-accurate seam — taking a 1-2px-smaller
        // near-tie instead would leave a thin band of doubled content.
        let fine_start = (coarse_best_shift - stride).max(-reverse_limit);
        let fine_end = (coarse_best_shift + stride).min(forward_limit);
        let mut best_shift = coarse_best_shift;
        let mut best_score = f32::MIN;
        let mut second_score = -2.0f32;
        for shift in fine_start..=fine_end {
            let score = Self::zncc_score_for_shift(config, pair, region, shift, content_blocks, 1);
            if score > best_score {
                second_score = best_score;
                best_score = score;
                best_shift = shift;
            } else if score > second_score {
                second_score = score;
            }
        }

        if best_score < -0.99 {
            return None;
        }

        if second_score < -0.99 {
            second_score = best_score;
        }

        Some((best_shift, best_score, second_score))
    }

    /// Score a candidate vertical shift by correlating the ENTIRE overlapping
    /// region of the two frames — every row in the overlap, across all textured
    /// columns — with a single normalized cross-correlation (ZNCC).
    ///
    /// This is the trust metric the append decision hinges on. A correct scroll
    /// offset lines the same pixels up and scores ~1.0; any wrong offset scores
    /// low. The previous implementation only sampled a few tiny 8px patches, a
    /// proxy so weak that true and false shifts scored alike — which forced the
    /// confidence threshold so low that wrong matches slipped through and
    /// produced duplicated bands in the stitched image.
    fn zncc_score_for_shift(
        config: StitchConfig,
        pair: FramePairRef<'_>,
        region: StitchRegion,
        shift: i32,
        content_blocks: &[usize],
        stride: usize,
    ) -> f32 {
        let valid_h = region.valid_height() as i32;
        let overlap = valid_h - shift.abs();
        if overlap <= config.min_overlap as i32 {
            return -1.0;
        }

        let block = config.dynamic_block_size.max(8);
        let stride = stride.max(1);
        // For a shift, `prev` row (fixed_top + prev_off + i) aligns with `next`
        // row (fixed_top + next_off + i) for i in 0..overlap.
        let prev_off = if shift >= 0 { shift } else { 0 } as usize;
        let next_off = if shift >= 0 { 0 } else { -shift } as usize;
        let base = region.fixed_top;
        let overlap = overlap as usize;

        let mut sum_p = 0.0f32;
        let mut sum_n = 0.0f32;
        let mut sum_pp = 0.0f32;
        let mut sum_nn = 0.0f32;
        let mut sum_pn = 0.0f32;
        let mut count = 0usize;

        let mut i = 0usize;
        while i < overlap {
            let row_p = (base + prev_off + i) * region.width;
            let row_n = (base + next_off + i) * region.width;
            for &b in content_blocks {
                let x0 = b * block;
                let x1 = ((b + 1) * block).min(region.width);
                let mut x = x0;
                while x < x1 {
                    let p = pair.prev[row_p + x];
                    let n = pair.next[row_n + x];
                    sum_p += p;
                    sum_n += n;
                    sum_pp += p * p;
                    sum_nn += n * n;
                    sum_pn += p * n;
                    count += 1;
                    x += stride;
                }
            }
            i += stride;
        }

        if count == 0 {
            return -1.0;
        }

        let n = count as f32;
        let cov = sum_pn - (sum_p * sum_n / n);
        let var_p = (sum_pp - (sum_p * sum_p / n)).max(0.0);
        let var_n = (sum_nn - (sum_n * sum_n / n)).max(0.0);
        // If either side is (near-)flat there is no correlation signal, so the
        // normalized score is undefined. Return 0 rather than dividing by a
        // clamped epsilon, which would blow a nonzero covariance up to a huge
        // spurious value and corrupt peak selection.
        let var_floor = n * 1e-3;
        if var_p < var_floor || var_n < var_floor {
            return 0.0;
        }
        let score = cov / (var_p.sqrt() * var_n.sqrt());
        score.clamp(-1.0, 1.0)
    }

    fn find_smart_seam(&self, pair: FramePairRef<'_>, region: StitchRegion, overlap_valid: usize, content_blocks: &[usize]) -> usize {
        let block = self.config.dynamic_block_size.max(8);
        let search_start = overlap_valid / self.config.seam_margin_divisor.max(2) as usize;
        let search_end =
            overlap_valid.saturating_mul((self.config.seam_margin_divisor.max(2) - 1) as usize) / self.config.seam_margin_divisor.max(2) as usize;

        let mut best_k = overlap_valid / 2;
        let mut best_energy = f32::MAX;

        for k in search_start..search_end.max(search_start + 1) {
            let prev_row = region.fixed_top + k;
            let next_row = k;
            if prev_row == 0 || prev_row >= region.height || next_row == 0 || next_row >= region.height {
                continue;
            }

            let mut energy = 0.0f32;
            let mut n = 0usize;
            for &b in content_blocks {
                let x0 = b * block;
                let x1 = ((b + 1) * block).min(region.width);
                for x in x0..x1 {
                    let p_idx = prev_row * region.width + x;
                    let p_up = (prev_row - 1) * region.width + x;
                    let n_idx = next_row * region.width + x;
                    let n_up = (next_row - 1) * region.width + x;

                    let p_grad = (pair.prev[p_idx] - pair.prev[p_up]).abs();
                    let n_grad = (pair.next[n_idx] - pair.next[n_up]).abs();
                    energy += (p_grad - n_grad).abs();
                    n += 1;
                }
            }

            if n == 0 {
                continue;
            }
            let avg = energy / n as f32;
            if avg < best_energy {
                best_energy = avg;
                best_k = k;
            }
        }

        best_k
    }

    fn execute_stitch(&mut self, new_image: &RgbaImage, plan: StitchAppendPlan) -> bool {
        let Some(canvas) = self.canvas.as_mut() else {
            return false;
        };

        let previous_valid_height = self.valid_height;
        let content_end = self.valid_height.saturating_sub(self.last_footer_height);
        if plan.trim_amount > content_end {
            return false;
        }

        let keep_h = content_end - plan.trim_amount;
        let append_h = plan.append_end_y.saturating_sub(plan.append_start_y);
        let new_total_h = keep_h + append_h;

        if new_total_h > canvas.height() {
            let new_cap = (canvas.height() * 2).max(new_total_h + CANVAS_RESIZE_HEADROOM);
            let width = canvas.width();
            let mut new_canvas = RgbaImage::new(width, new_cap);
            Self::copy_region(canvas, 0, &mut new_canvas, 0, keep_h);
            self.canvas = Some(new_canvas);
        }

        let Some(canvas) = self.canvas.as_mut() else {
            return false;
        };

        if append_h > 0 {
            Self::copy_region(new_image, plan.append_start_y, canvas, keep_h, append_h);
        }

        self.valid_height = new_total_h;
        self.last_footer_height = plan.fixed_bottom;
        self.thumbnail.mark_dirty_from(keep_h.min(previous_valid_height));
        true
    }

    fn copy_region(src: &RgbaImage, src_y: u32, dest: &mut RgbaImage, dest_y: u32, height: u32) {
        if height == 0 {
            return;
        }
        let width_bytes = (src.width() * 4) as usize;
        let copy_bytes = height as usize * width_bytes;
        let src_offset = src_y as usize * width_bytes;
        let dest_offset = dest_y as usize * width_bytes;

        let src_raw = src.as_raw();
        let dest_raw: &mut [u8] = dest.as_mut();

        if src_offset + copy_bytes <= src_raw.len() && dest_offset + copy_bytes <= dest_raw.len() {
            dest_raw[dest_offset..dest_offset + copy_bytes].copy_from_slice(&src_raw[src_offset..src_offset + copy_bytes]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage, imageops};

    fn crop_frame(source: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(source, 0, y, source.width(), h).to_image()
    }

    /// A text-like page: white background, sparse dark "glyphs" on periodic
    /// text lines, with whitespace left/right margins — the layout of a
    /// typical web article or chat log. Every text line gets a distinct glyph
    /// pattern so rows are not self-similar (as with real anti-aliased text).
    fn text_page(width: u32, height: u32) -> RgbaImage {
        let mut page = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
        let margin = width / 10;
        for y in 0..height {
            // Denser text: 20px line pitch, 16px glyph band (like body copy).
            if (y % 20) < 16 {
                let line = y / 20;
                for x in margin..(width - margin) {
                    let h = (x.wrapping_mul(2_654_435_761)) ^ (y.wrapping_mul(40_503)) ^ (line.wrapping_mul(2_246_822_519));
                    if (h & 0x3) < 2 {
                        let v = (h >> 8 & 0x3f) as u8;
                        page.put_pixel(x, y, Rgba([v, v, v, 255]));
                    }
                }
            }
        }
        page
    }

    #[test]
    fn stitcher_reconstructs_full_page_on_scroll() {
        // Regression: text pages have whitespace margins, so the old
        // "unchanged column" selector kept only the (flat) margins and
        // rejected every frame after the first, capturing just one viewport.
        // The old sticky-region detector also mistook scrolling whitespace for
        // a fixed footer and trimmed real content. With both fixed, a full
        // top-to-bottom scroll must reconstruct far more than one viewport.
        let page_w = 300u32;
        let page_h = 1800u32;
        let view_h = 300u32;
        let page = text_page(page_w, page_h);

        for step in [4u32, 8, 12] {
            let mut stitcher = ScrollStitcher::new();
            let mut y = 0u32;
            let mut guard = 0;
            loop {
                let _ = stitcher.process_frame_detailed(crop_frame(&page, y, view_h));
                if y + view_h >= page_h {
                    break;
                }
                y = (y + step).min(page_h - view_h);
                guard += 1;
                assert!(guard < 5000, "scroll loop failed to terminate");
            }

            let final_img = stitcher.get_final_image().expect("final image");
            // Must reconstruct well beyond a single viewport. (A lossy stitcher
            // drops some frames at speed, so we require solid coverage rather
            // than the full page height.)
            assert!(
                final_img.height() >= view_h * 2,
                "step={step}: expected >= {} rows (much more than one viewport), got {}",
                view_h * 2,
                final_img.height()
            );
        }
    }

    #[test]
    fn detect_content_blocks_skips_flat_margins() {
        // Whitespace margins carry no edge energy and must be excluded, while
        // the textured interior columns are kept.
        let page = text_page(300, 400);
        let analysis = FrameAnalysis::from_image(&page);
        let region = StitchRegion {
            width: analysis.width,
            height: analysis.height,
            fixed_top: 0,
            fixed_bottom: 0,
        };
        let mut blocks = Vec::new();
        ScrollStitcher::detect_content_blocks(StitchConfig::default(), &analysis.edge, &analysis.edge, region, &mut blocks);

        assert!(blocks.len() >= StitchConfig::default().min_content_blocks);
        // The far-left margin block (block 0) is flat white and must be dropped.
        assert!(!blocks.contains(&0), "flat left margin should not be a content block");
    }

    #[test]
    #[ignore = "timing benchmark; run explicitly with --ignored"]
    fn bench_process_frame_realistic_size() {
        // Real capture frames are ~1492x586 on a 2x display. Verify a single
        // process_frame_detailed stays well under the ~16ms frame budget.
        let page = text_page(1492, 4000);
        let mut stitcher = ScrollStitcher::new();
        let _ = stitcher.process_frame_detailed(crop_frame(&page, 0, 586));
        let iters = 20u32;
        let start = std::time::Instant::now();
        for k in 0..iters {
            let y = 40 + k * 12;
            let _ = stitcher.process_frame_detailed(crop_frame(&page, y, 586));
        }
        let per = start.elapsed().as_secs_f64() * 1000.0 / f64::from(iters);
        eprintln!("BENCH process_frame avg = {per:.2} ms/frame (budget ~16ms)");
        assert!(per < 16.0, "per-frame cost {per:.2}ms exceeds frame budget");
    }

    #[test]
    fn zncc_scores_true_shift_high_and_wrong_shift_low() {
        // A correct scroll offset must score near 1.0 (identical overlapping
        // pixels), and an incorrect offset must score much lower — that gap is
        // what lets the append gate reject duplicate-producing wrong matches.
        let page = text_page(240, 900);
        let prev = crop_frame(&page, 0, 300);
        let next = crop_frame(&page, 20, 300); // true shift = 20

        let prev_a = FrameAnalysis::from_image(&prev);
        let next_a = FrameAnalysis::from_image(&next);
        let region = StitchRegion {
            width: prev_a.width,
            height: prev_a.height,
            fixed_top: 0,
            fixed_bottom: 0,
        };
        let mut blocks = Vec::new();
        ScrollStitcher::detect_content_blocks(StitchConfig::default(), &prev_a.edge, &next_a.edge, region, &mut blocks);
        let pair = FramePairRef {
            prev: &prev_a.gray,
            next: &next_a.gray,
        };
        let good = ScrollStitcher::zncc_score_for_shift(StitchConfig::default(), pair, region, 20, &blocks, 1);
        let bad = ScrollStitcher::zncc_score_for_shift(StitchConfig::default(), pair, region, 137, &blocks, 1);
        eprintln!("ZNCC true(20)={good:.3} wrong(137)={bad:.3}");
        assert!(good > 0.9, "true shift should score high, got {good:.3}");
        assert!(good - bad > 0.3, "true shift must clearly beat a wrong shift ({good:.3} vs {bad:.3})");
    }

    #[test]
    fn stitcher_appends_on_forward_scroll() {
        let mut stitcher = ScrollStitcher::new();
        let src = text_page(240, 900);

        // Feed a short sequence of small forward scrolls, as the live capture
        // loop does (~16ms frames). The canvas must grow past the first frame.
        assert_eq!(stitcher.process_frame_detailed(crop_frame(&src, 0, 300)).status, StitchFrameStatus::Appended);
        let mut appended = 0;
        for y in [8u32, 16, 24, 32, 40] {
            if stitcher.process_frame_detailed(crop_frame(&src, y, 300)).status == StitchFrameStatus::Appended {
                appended += 1;
            }
        }
        assert!(appended >= 3, "expected most small scrolls to append, got {appended}/5");
        let (_, height) = stitcher.current_image().expect("canvas");
        assert!(height > 300, "canvas should extend past the first frame, got {height}");
    }

    #[test]
    fn stitcher_marks_stationary_for_same_frame() {
        let mut stitcher = ScrollStitcher::new();
        let src = text_page(240, 900);
        let first = crop_frame(&src, 40, 300);
        assert_eq!(stitcher.process_frame_detailed(first.clone()).status, StitchFrameStatus::Appended);
        assert_eq!(stitcher.process_frame_detailed(first).status, StitchFrameStatus::Stationary);
    }

    /// A non-periodic page: every row has a distinct random texture with no
    /// repeating pitch, so a given shift matches at exactly one offset. Real
    /// screen content (unique glyphs, varied layout) behaves this way; use this
    /// where period-aliasing of the synthetic `text_page` would confound the
    /// test (e.g. verifying reverse detection).
    fn unique_page(width: u32, height: u32) -> RgbaImage {
        let mut page = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
        let margin = width / 10;
        for y in 0..height {
            for x in margin..(width - margin) {
                let h = (x.wrapping_mul(2_654_435_761)) ^ (y.wrapping_mul(2_246_822_519)).wrapping_add(y.wrapping_mul(y));
                if (h & 0x3) < 2 {
                    let v = (h >> 8 & 0x3f) as u8;
                    page.put_pixel(x, y, Rgba([v, v, v, 255]));
                }
            }
        }
        page
    }

    #[test]
    fn thumbnail_keeps_growing_as_canvas_grows() {
        // The preview thumbnail must keep getting taller as more of the page is
        // stitched — regression for "preview freezes while px counter climbs".
        let page = unique_page(300, 6000);
        let view_h = 300u32;
        let mut stitcher = ScrollStitcher::new();
        let _ = stitcher.process_frame_detailed(crop_frame(&page, 0, view_h));

        let mut heights = Vec::new();
        let mut y = 0u32;
        for _ in 0..120 {
            y = (y + 12).min(6000 - view_h);
            let _ = stitcher.process_frame_detailed(crop_frame(&page, y, view_h));
            if let Some(t) = stitcher.make_thumbnail(500) {
                heights.push(t.height());
            }
            if y + view_h >= 6000 {
                break;
            }
        }

        let first = *heights.first().expect("some thumbnails");
        let last = *heights.last().expect("some thumbnails");
        assert!(last > first, "thumbnail should grow taller ({first} -> {last})");
        // Monotonic non-decreasing.
        for w in heights.windows(2) {
            assert!(w[1] >= w[0], "thumbnail height regressed: {} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn stitcher_reports_reverse() {
        let mut stitcher = ScrollStitcher::new();
        let src = unique_page(240, 900);
        // Scroll forward, then a small reverse step (within the reverse search
        // range) must be recognized and pause the capture rather than append.
        assert_eq!(stitcher.process_frame_detailed(crop_frame(&src, 60, 300)).status, StitchFrameStatus::Appended);
        let detail = stitcher.process_frame_detailed(crop_frame(&src, 54, 300));
        assert!(matches!(detail.status, StitchFrameStatus::Reverse | StitchFrameStatus::LowConfidence));
    }

    #[test]
    fn slow_sub_threshold_scroll_still_accumulates() {
        // Regression: a slow drag advances only 1-2px per frame — below
        // min_scroll_threshold. If each Stationary frame reset the reference,
        // that motion would never accumulate and the canvas would freeze at the
        // first frame (the "preview never grows" bug). Holding the reference
        // across sub-threshold frames must let the offset build up and append.
        let page_h = 900u32;
        let view_h = 300u32;
        let page = text_page(240, page_h);

        let mut stitcher = ScrollStitcher::new();
        assert_eq!(stitcher.process_frame_detailed(crop_frame(&page, 0, view_h)).status, StitchFrameStatus::Appended);

        // Advance 1px at a time (well under the 4px threshold).
        let mut saw_append = false;
        for y in 1..=40u32 {
            if stitcher.process_frame_detailed(crop_frame(&page, y, view_h)).status == StitchFrameStatus::Appended {
                saw_append = true;
            }
        }

        assert!(saw_append, "slow 1px scrolls must eventually append");
        let (_, height) = stitcher.current_image().expect("canvas");
        assert!(height > view_h, "canvas should grow past the first frame, got {height}");
    }
}

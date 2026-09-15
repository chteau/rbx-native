//! The order the post passes are actually encoded in: the bloom pyramid up and
//! back down, the full-frame blur chain `BlurEffect` and `DepthOfFieldEffect`
//! share, and the resolve that writes the final image.
//!
//! Lives apart from `renderer::post` only to keep that file inside the
//! workspace's 400-line guideline; [`Post::resolve`] is written against `Post`'s
//! own private state and belongs to it.

use super::pipelines::pass;
use super::targets::Level;
use super::Post;

impl Post {
    /// Builds the bloom and full-frame blur chains where each is enabled, then
    /// writes the graded, tone-mapped frame to `target`.
    ///
    /// The resolve always runs — it is the only pass that writes the final
    /// image — but the chains in front of it are skipped where nothing would
    /// come of them: the resolve reads whatever an untouched (so zeroed or
    /// stale) first level holds, and gates both on the uniform's own flags.
    pub(in crate::renderer) fn resolve(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let Some(first) = targets.chain.first() else {
            return;
        };

        if self.intensity() > 0.0 {
            pass(
                encoder,
                "rbxview bloom threshold",
                &first.view,
                &self.threshold,
                &[&self.uniform_bind, &targets.scene_source],
                true,
            );
            self.downsample_chain(encoder, "rbxview bloom downsample", &targets.chain);
            // Back down the chain, each level adding its own blur into the
            // wider one below it.
            for level in (1..targets.chain.len()).rev() {
                pass(
                    encoder,
                    "rbxview bloom upsample",
                    &targets.chain[level - 1].view,
                    &self.upsample,
                    &[&self.uniform_bind, &targets.chain[level].source],
                    false,
                );
            }
        }

        // The full-frame blur reuses `fs_downsample` with no threshold, seeded
        // straight from the sharp scene rather than from `first.view`: unlike
        // bloom, it never walks back up the chain — the deepest, blurriest
        // level is sampled directly in the resolve, where the sampler's own
        // bilinear filtering stretches it back to the frame's size. That is
        // cheaper than a tent-upsample walk and reads as a legitimate blur
        // rather than bloom's glow, which needs the walk to stay soft-edged.
        //
        // Which level the resolve then reads is `targets.blur_depth`'s business,
        // not this one's: an empty chain means neither a `BlurEffect` nor a
        // `DepthOfFieldEffect` is enabled, and `fs_resolve` samples nothing out of
        // that group at all.
        if let Some(seed) = targets.blur_chain.first() {
            pass(
                encoder,
                "rbxview blur seed",
                &seed.view,
                &self.downsample,
                &[&self.uniform_bind, &targets.scene_source],
                true,
            );
            self.downsample_chain(encoder, "rbxview blur downsample", &targets.blur_chain);
        }

        pass(
            encoder,
            "rbxview resolve",
            target,
            // The depth buffer rides in the group below, so the resolve built for
            // that shape of it is the one that can bind it; nothing else about
            // the two pipelines differs.
            if self.samples > 1 {
                &self.resolve_multisampled
            } else {
                &self.resolve
            },
            &[
                &self.uniform_bind,
                &targets.scene_source,
                &first.source,
                &targets.blur_depth,
            ],
            true,
        );
    }

    /// Downsamples `chain[0]` into each following level in turn — the part of
    /// the pyramid bloom and the full-frame blur build identically, once each
    /// has its own way of seeding `chain[0]`.
    fn downsample_chain(&self, encoder: &mut wgpu::CommandEncoder, label: &str, chain: &[Level]) {
        for level in 1..chain.len() {
            pass(
                encoder,
                label,
                &chain[level].view,
                &self.downsample,
                &[&self.uniform_bind, &chain[level - 1].source],
                true,
            );
        }
    }
}

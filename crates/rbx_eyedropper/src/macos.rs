use std::cell::RefCell;

use block2::RcBlock;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

pub(crate) fn pick(done: Box<dyn FnOnce(crate::Outcome) + Send>) {
    if MainThreadMarker::new().is_none() {
        done(Err(
            "The colour picker has to start on the main thread.".into()
        ));
        return;
    }
    let sampler = NSColorSampler::new();
    let keep_alive = sampler.clone();
    let done = RefCell::new(Some(done));
    let handler = RcBlock::new(move |color: *mut NSColor| {
        let _sampler = &keep_alive;
        // SAFETY: AppKit passes either null or a valid colour for the call.
        let color = unsafe { color.as_ref() };
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        let outcome = match color {
            None => Ok(None),
            Some(color) => {
                let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
                match srgb {
                    Some(c) => {
                        // SAFETY: each selector returns a CGFloat (f64 on 64-bit).
                        let (r, g, b): (f64, f64, f64) = unsafe {
                            (
                                msg_send![&*c, redComponent],
                                msg_send![&*c, greenComponent],
                                msg_send![&*c, blueComponent],
                            )
                        };
                        Ok(Some(crate::from_unit(r, g, b)))
                    }
                    None => Err("The picked colour could not be converted to sRGB.".into()),
                }
            }
        };
        done(outcome);
    });
    // SAFETY: the handler matches the selector's block signature and the
    // sampler is kept alive by the block itself.
    unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
}

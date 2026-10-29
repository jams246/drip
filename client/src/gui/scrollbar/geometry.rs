use gpui::{Pixels, px};

#[derive(Clone, Copy, Debug)]
pub(super) struct Geometry {
    pub viewport: Pixels,
    pub maximum: Pixels,
    pub offset: Pixels,
    pub height: Pixels,
    pub top: Pixels,
}

pub(super) fn clamp(offset: Pixels, maximum: Pixels) -> Pixels {
    offset.clamp(-maximum.max(px(0.)), px(0.))
}

impl Geometry {
    pub fn new(viewport: Pixels, maximum: Pixels, offset: Pixels) -> Option<Self> {
        if viewport <= px(0.) || maximum <= px(0.) {
            return None;
        }
        let offset = clamp(offset, maximum);
        let height = (viewport * (viewport / (viewport + maximum)))
            .max(px(24.))
            .min(viewport);
        let top = (viewport - height) * (-offset / maximum);
        Some(Self {
            viewport,
            maximum,
            offset,
            height,
            top,
        })
    }

    pub fn page(self, pointer: Pixels) -> Pixels {
        let delta = if pointer < self.top {
            self.viewport
        } else if pointer > self.top + self.height {
            -self.viewport
        } else {
            px(0.)
        };
        clamp(self.offset + delta, self.maximum)
    }

    pub fn drag(self, pointer: Pixels, grab: Pixels) -> Pixels {
        let travel = self.viewport - self.height;
        if travel <= px(0.) {
            return self.offset;
        }
        -self.maximum * ((pointer - grab).clamp(px(0.), travel) / travel)
    }
}

#[cfg(test)]
mod tests;

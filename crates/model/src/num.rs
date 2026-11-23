use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A JSON number as JavaScript sees it: one `f64`.
///
/// Whole values are written without a decimal point (`5`, not `5.0`), as
/// `JSON.stringify` does, so a file round-trips byte for byte and a sync client
/// does not see a change that is not there.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub struct Num(pub f64);

/// Largest integer an `f64` holds exactly (2^53).
const MAX_SAFE: f64 = 9_007_199_254_740_992.0;

impl Num {
    pub fn get(self) -> f64 {
        self.0
    }
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Num(v)
    }
}

impl From<i64> for Num {
    #[allow(clippy::cast_precision_loss)]
    fn from(v: i64) -> Self {
        Num(v as f64)
    }
}

impl Serialize for Num {
    #[allow(clippy::cast_possible_truncation)]
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        if v.fract() == 0.0 && v.abs() <= MAX_SAFE { s.serialize_i64(v as i64) } else { s.serialize_f64(v) }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Num)
    }
}

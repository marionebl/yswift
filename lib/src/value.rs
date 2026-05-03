use crate::array::YrsArray;
use crate::map::YrsMap;
use crate::text::YrsText;
use std::sync::Arc;
use yrs::types::Value as YrsTypeValue;

/// A typed value pulled out of a Y collection.
///
/// `Scalar` carries a JSON-encoded primitive (string / number / bool / null /
/// JSON-array / JSON-object) — same shape that the existing scalar `get`
/// methods on `YrsMap` / `YrsArray` return.
///
/// The remaining variants carry **live handles** to nested CRDT collections,
/// matching the JS `Y.Map.get(key) -> Y.Map | Y.Array | Y.Text` semantics. The
/// returned handle observes/mutates the same underlying CRDT branch as the
/// parent — there is no copy.
///
/// This is an internal Rust enum. The FFI boundary cannot carry Object-typed
/// enum variants in uniffi's UDL backend, so we surface the variants as four
/// sibling getters on `YrsMap` / `YrsArray` plus a discriminator
/// `YrsValueKind`. Swift code recomposes them into a Swift-side `YValue`.
pub(crate) enum YrsValue {
    Scalar { json: String },
    YMap { value: Arc<YrsMap> },
    YArray { value: Arc<YrsArray> },
    YText { value: Arc<YrsText> },
}

/// FFI-exposed discriminator. UDL `enum YrsValueKind { ... }` maps onto this.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum YrsValueKind {
    Scalar,
    YMap,
    YArray,
    YText,
}

impl std::fmt::Debug for YrsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YrsValue::Scalar { json } => write!(f, "Scalar({})", json),
            YrsValue::YMap { .. } => write!(f, "YMap(<handle>)"),
            YrsValue::YArray { .. } => write!(f, "YArray(<handle>)"),
            YrsValue::YText { .. } => write!(f, "YText(<handle>)"),
        }
    }
}

impl YrsValue {
    pub(crate) fn kind(&self) -> YrsValueKind {
        match self {
            YrsValue::Scalar { .. } => YrsValueKind::Scalar,
            YrsValue::YMap { .. } => YrsValueKind::YMap,
            YrsValue::YArray { .. } => YrsValueKind::YArray,
            YrsValue::YText { .. } => YrsValueKind::YText,
        }
    }

    /// Convert a `yrs::types::Value` (the upstream sum type returned by `Map::get`
    /// / `Array::get`) into our internal `YrsValue`.
    ///
    /// Scalars are JSON-encoded so they round-trip through the existing
    /// stringly-typed Swift coder (`Sources/YSwift/Coder.swift`). Nested types
    /// are wrapped in `Arc` so callers can keep mutating them after the parent
    /// transaction closes.
    pub(crate) fn from_yrs_value(value: YrsTypeValue) -> Self {
        match value {
            YrsTypeValue::Any(any) => {
                let mut buf = String::new();
                any.to_json(&mut buf);
                YrsValue::Scalar { json: buf }
            }
            YrsTypeValue::YMap(map_ref) => YrsValue::YMap {
                value: Arc::new(YrsMap::from(map_ref)),
            },
            YrsTypeValue::YArray(array_ref) => YrsValue::YArray {
                value: Arc::new(YrsArray::from(array_ref)),
            },
            YrsTypeValue::YText(text_ref) => YrsValue::YText {
                value: Arc::new(YrsText::from(text_ref)),
            },
            // XmlElement / XmlFragment / XmlText / Doc / WeakRef are not part
            // of the preperoni schema and out of scope for this patch. Map
            // them to a `Scalar { json: "null" }` so callers see a recognisable
            // sentinel rather than a panic — nested-traversal-for-our-schema
            // is the only promised feature here.
            _ => YrsValue::Scalar {
                json: "null".to_string(),
            },
        }
    }
}

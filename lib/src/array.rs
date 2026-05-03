use crate::map::YrsMap;
use crate::subscription::YSubscription;
use crate::text::YrsText;
use crate::transaction::YrsTransaction;
use crate::value::{YrsValue, YrsValueKind};
use crate::{change::YrsChange, error::CodingError};
use std::cell::RefCell;
use std::fmt::Debug;
use std::sync::Arc;
use yrs::types::{array::ArrayPrelim, map::MapPrelim, text::TextPrelim};
use yrs::{types::Value, Any, Array, ArrayRef, Observable};
use yrs::branch::Branch;
use crate::doc::YrsCollectionPtr;

pub(crate) struct YrsArray(RefCell<ArrayRef>);

unsafe impl Send for YrsArray {}
unsafe impl Sync for YrsArray {}

impl AsRef<Branch> for YrsArray {
    fn as_ref(&self) -> &Branch {
        //FIXME: after yrs v0.18 use logical references
        let branch = &*self.0.borrow();
        unsafe { std::mem::transmute(branch.as_ref()) }
    }
}

impl From<ArrayRef> for YrsArray {
    fn from(value: ArrayRef) -> Self {
        YrsArray(RefCell::from(value))
    }
}
pub(crate) trait YrsArrayEachDelegate: Send + Sync + Debug {
    fn call(&self, value: String);
}

pub(crate) trait YrsArrayObservationDelegate: Send + Sync + Debug {
    fn call(&self, value: Vec<YrsChange>);
}

// unsafe impl Send for YrsArrayIterator {}
// unsafe impl Sync for YrsArrayIterator {}

// pub(crate) struct YrsArrayIterator {
//     inner: RefCell<ArrayIter<&'static YrsTransaction, YrsTransaction>>,
// }

// impl YrsArrayIterator {
//     pub(crate) fn next(&self) -> Option<String> {
//         let val = self.inner.borrow_mut().next();

//         match val {
//             Some(val) => {
//                 let mut buf = String::new();
//                 if let Value::Any(any) = val {
//                     any.to_json(&mut buf);
//                     Some(buf)
//                 } else {
//                     // @TODO: fix silly handling, it will just call it with nil if casting fails
//                     None
//                 }
//             }
//             None => None,
//         }
//     }
// }

impl YrsArray {
    // pub(crate) fn iter(&self, txn: &'static YrsTransaction) -> Arc<YrsArrayIterator> {
    //     let arr = self.0.borrow();
    //     Arc::new(YrsArrayIterator {
    //         inner: RefCell::new(arr.iter(txn)),
    //     })
    // }
    pub(crate) fn raw_ptr(&self) -> YrsCollectionPtr {
        let borrowed = self.0.borrow();
        YrsCollectionPtr::from(borrowed.as_ref())
    }

    pub(crate) fn each(
        &self,
        transaction: &YrsTransaction,
        delegate: Box<dyn YrsArrayEachDelegate>,
    ) {
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        let arr = self.0.borrow();
        arr.iter(tx).for_each(|val| {
            let mut buf = String::new();
            if let Value::Any(any) = val {
                any.to_json(&mut buf);
                delegate.call(buf);
            } else {
                // @TODO: fix silly handling, it will just call with empty string if casting fails
                delegate.call(buf);
            }
        });
    }

    pub(crate) fn get(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Result<String, CodingError> {
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let arr = self.0.borrow();
        if let Some(value) = arr.get(tx, index) {
            let mut buf = String::new();
            if let Value::Any(any) = value {
                any.to_json(&mut buf);
                Ok(buf)
            } else {
                Err(CodingError::EncodingError)
            }
        } else {
            // Actually there is no element here, so it shouldn't be EncodingErro
            Err(CodingError::EncodingError)
        }
    }

    /// Typed read that surfaces nested CRDT collections as live handles.
    /// Mirrors JS `Y.Array.get(i)` semantics; see `YrsMap::get_value`.
    /// Internal helper — FFI surface is the four sibling methods below.
    pub(crate) fn get_value(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<YrsValue> {
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let arr = self.0.borrow();
        arr.get(tx, index).map(YrsValue::from_yrs_value)
    }

    /// FFI: discriminator at `index`, or None when out of range.
    pub(crate) fn value_kind_at_index(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<YrsValueKind> {
        self.get_value(transaction, index).map(|v| v.kind())
    }

    pub(crate) fn get_scalar_at_index(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<String> {
        match self.get_value(transaction, index)? {
            YrsValue::Scalar { json } => Some(json),
            _ => None,
        }
    }

    pub(crate) fn get_map_at_index(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<Arc<YrsMap>> {
        match self.get_value(transaction, index)? {
            YrsValue::YMap { value } => Some(value),
            _ => None,
        }
    }

    pub(crate) fn get_array_at_index(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<Arc<YrsArray>> {
        match self.get_value(transaction, index)? {
            YrsValue::YArray { value } => Some(value),
            _ => None,
        }
    }

    pub(crate) fn get_text_at_index(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Option<Arc<YrsText>> {
        match self.get_value(transaction, index)? {
            YrsValue::YText { value } => Some(value),
            _ => None,
        }
    }

    /// Inserts an empty nested `Y.Map` at `index` and returns a live handle.
    pub(crate) fn insert_map(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Arc<YrsMap> {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted = parent.insert(tx, index, MapPrelim::<Any>::new());
        Arc::new(YrsMap::from(inserted))
    }

    /// Inserts an empty nested `Y.Array` at `index` and returns a live handle.
    pub(crate) fn insert_array(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Arc<YrsArray> {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted = parent.insert(tx, index, ArrayPrelim::<[Any; 0], Any>::from([]));
        Arc::new(YrsArray::from(inserted))
    }

    /// Inserts an empty nested `Y.Text` at `index` and returns a live handle.
    pub(crate) fn insert_text(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Arc<YrsText> {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted = parent.insert(tx, index, TextPrelim::new(""));
        Arc::new(YrsText::from(inserted))
    }

    pub(crate) fn insert(&self, transaction: &YrsTransaction, index: u32, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();

        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.insert(tx, index, avalue);
    }

    pub(crate) fn insert_range(
        &self,
        transaction: &YrsTransaction,
        index: u32,
        values: Vec<String>,
    ) {
        let arr = self.0.borrow_mut();
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let add_values: Vec<Any> = values
            .into_iter()
            .map(|value| Any::from_json(value.as_str()).unwrap())
            .collect();

        arr.insert_range(tx, index, add_values)
    }

    pub(crate) fn length(&self, transaction: &YrsTransaction) -> u32 {
        let arr = self.0.borrow();
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        arr.len(tx)
    }

    pub(crate) fn push_back(&self, transaction: &YrsTransaction, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        self.0.borrow_mut().push_back(tx, avalue);
    }

    pub(crate) fn push_front(&self, transaction: &YrsTransaction, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();

        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.push_front(tx, avalue);
    }

    pub(crate) fn remove(&self, transaction: &YrsTransaction, index: u32) {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.remove(tx, index)
    }

    pub(crate) fn remove_range(&self, transaction: &YrsTransaction, index: u32, len: u32) {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.remove_range(tx, index, len)
    }

    pub(crate) fn observe(&self, delegate: Box<dyn YrsArrayObservationDelegate>) -> Arc<YSubscription> {
        let subscription = self
            .0
            .borrow_mut()
            .observe(move |transaction, text_event| {
                let delta = text_event.delta(transaction);
                let result: Vec<YrsChange> =
                    delta.iter().map(|change| YrsChange::from(change)).collect();
                delegate.call(result)
            });

            Arc::new(YSubscription::new(subscription))
    }

    pub(crate) fn to_a(&self, transaction: &YrsTransaction) -> Vec<String> {
        let arr = self.0.borrow();
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        let arr = arr
            .iter(tx)
            .filter_map(|v| {
                let mut buf = String::new();
                if let Value::Any(any) = v {
                    any.to_json(&mut buf);
                    Some(buf)
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

        arr
    }
}

#[cfg(test)]
mod tests {
    use crate::value::YrsValue;
    use crate::YrsDoc;

    #[test]
    fn array_insert_nested_map_returns_handle_and_get_value_round_trips() {
        let doc = YrsDoc::new();
        let arr = doc.get_array("rows".to_string());
        let txn = doc.transact(None);

        let row = arr.insert_map(&txn, 0);
        row.insert(&txn, "name".to_string(), "\"Pasta\"".to_string());

        let pulled = arr.get_value(&txn, 0).unwrap();
        match pulled {
            YrsValue::YMap { value } => {
                assert_eq!(value.length(&txn), 1);
            }
            other => panic!("expected YMap, got {:?}", other),
        }
    }

    #[test]
    fn array_insert_nested_array_returns_handle() {
        let doc = YrsDoc::new();
        let outer = doc.get_array("outer".to_string());
        let txn = doc.transact(None);
        let inner = outer.insert_array(&txn, 0);
        inner.insert(&txn, 0, "\"a\"".to_string());

        let pulled = outer.get_value(&txn, 0).unwrap();
        match pulled {
            YrsValue::YArray { value } => {
                assert_eq!(value.length(&txn), 1);
                assert_eq!(value.to_a(&txn), vec!["\"a\""]);
            }
            other => panic!("expected YArray, got {:?}", other),
        }
    }

    #[test]
    fn array_get_value_returns_scalar() {
        let doc = YrsDoc::new();
        let arr = doc.get_array("xs".to_string());
        let txn = doc.transact(None);
        arr.insert(&txn, 0, "42".to_string());
        let v = arr.get_value(&txn, 0).unwrap();
        match v {
            // Same `Any::to_json` integer-to-float behaviour as everywhere else.
            YrsValue::Scalar { json } => assert_eq!(json, "42.0"),
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn array_get_value_out_of_bounds_is_none() {
        let doc = YrsDoc::new();
        let arr = doc.get_array("xs".to_string());
        let txn = doc.transact(None);
        assert!(arr.get_value(&txn, 0).is_none());
    }

    #[test]
    fn array_insert_text_returns_handle() {
        let doc = YrsDoc::new();
        let arr = doc.get_array("xs".to_string());
        let txn = doc.transact(None);
        let t = arr.insert_text(&txn, 0);
        t.append(&txn, "hi".to_string());
        match arr.get_value(&txn, 0).unwrap() {
            YrsValue::YText { value } => assert_eq!(value.get_string(&txn), "hi"),
            other => panic!("expected YText, got {:?}", other),
        }
    }
}

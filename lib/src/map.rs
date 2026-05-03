use crate::array::YrsArray;
use crate::error::CodingError;
use crate::mapchange::{YrsEntryChange, YrsMapChange};
use crate::subscription::YSubscription;
use crate::text::YrsText;
use crate::transaction::YrsTransaction;
use crate::value::{YrsValue, YrsValueKind};
use std::cell::RefCell;
use std::fmt::Debug;
use std::sync::Arc;
use yrs::branch::Branch;
use yrs::types::{array::ArrayPrelim, map::MapPrelim, text::TextPrelim};
use yrs::Observable;
use yrs::{types::Value, Any, Map, MapRef};
use crate::doc::YrsCollectionPtr;

pub(crate) struct YrsMap(RefCell<MapRef>);

// Marks that this type can be transferred across thread boundaries.
unsafe impl Send for YrsMap {}
// Marks that this type is safe to share references between threads.
unsafe impl Sync for YrsMap {}

impl AsRef<Branch> for YrsMap {
    fn as_ref(&self) -> &Branch {
        //FIXME: after yrs v0.18 use logical references
        let branch = &*self.0.borrow();
        unsafe { std::mem::transmute(branch.as_ref()) }
    }
}

// Provides the implementation for the From trait, supporting
// converting from a MapRef type into a YrsMap type.
impl From<MapRef> for YrsMap {
    fn from(value: MapRef) -> Self {
        YrsMap(RefCell::from(value))
    }
}

// A representation of a callback that is invoked from the various
// map iterators, specifically to provide the JSON-string of the iterated
// value from the map (for example, with `values` or `iter`).
//
// This allows the outside code (Swift, for example) to
// handle the deserialization from JSON string into whatever the appropriate
// type is within the swift language bindings. The `keys` iterator doesn't
// need this "translation", while `values` does.
//
// The type is boxed and used as a dynamic type:
// `Box<dyn YrsMapIteratorDelegate>`
// rather than having the keys, values, or iter functions expose an iterator
// back to the external language bindings.
pub(crate) trait YrsMapIteratorDelegate: Send + Sync + Debug {
    fn call(&self, value: String);
}

pub(crate) trait YrsMapKVIteratorDelegate: Send + Sync + Debug {
    fn call(&self, key: String, value: String);
}

pub(crate) trait YrsMapObservationDelegate: Send + Sync + Debug {
    fn call(&self, value: Vec<YrsMapChange>);
}

/*
IMPL order:
- [X] [insert, len, contains_key]
- [X] [get, remove, clear]
- [X] [keys, values, iter]
- [ ] [observe, unobserve]
 */

impl YrsMap {
    pub(crate) fn raw_ptr(&self) -> YrsCollectionPtr {
        let borrowed = self.0.borrow();
        YrsCollectionPtr::from(borrowed.as_ref())
    }

    /// Inserts the key and value you provide into the map.
    pub(crate) fn insert(&self, transaction: &YrsTransaction, key: String, value: String) {
        // decodes the `value` as JSON and converts it into a lib0::Any enumeration
        let any_value = Any::from_json(value.as_str()).unwrap();

        // acquire a *mutable* transaction
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();

        // pull out a mutable reference to the YrsMap this type wraps
        let map = self.0.borrow_mut();
        // insert into the wrapped map.
        map.insert(tx, key, any_value);

        // Documentation note from YrsMap about inserting a preliminary type - for future
        // reference...
        // // insert nested shared type
        // let nested = map.insert(&mut txn, "key2", MapPrelim::from([("inner", "value2")]));
        // nested.insert(&mut txn, "inner2", 100);
    }

    /// Returns the size of the map.
    pub(crate) fn length(&self, transaction: &YrsTransaction) -> u32 {
        let map = self.0.borrow();
        // acquire a transaction, but we don't need to borrow it since we're
        // not mutating anything in this method.
        let binding = transaction.transaction();
        let tx = binding.as_ref().unwrap();
        // If we try and do the above on a single line, I get the error:
        // creates a temporary value which is freed while still in use

        map.len(tx)
    }

    /// Returns a Boolean value that indicates whether the map contains the key you provide.
    pub(crate) fn contains_key(&self, transaction: &YrsTransaction, key: String) -> bool {
        let map = self.0.borrow();
        // acquire a transaction, but we don't need to borrow it since we're
        // not mutating anything in this method.
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        map.contains_key(tx, key.as_str())
    }

    pub(crate) fn get(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Result<String, CodingError> {
        let binding = transaction.transaction();
        let tx = binding.as_ref().unwrap();
        let map = self.0.borrow();
        let v = map.get(tx, key.as_str()).unwrap();
        let mut buf = String::new();
        if let Value::Any(any) = v {
            any.to_json(&mut buf);
            Ok(buf)
        } else {
            Err(CodingError::EncodingError)
        }
    }

    /// Typed read that surfaces nested CRDT collections as live handles.
    ///
    /// Mirrors JS `Y.Map.get(key)`: scalars come back as `YrsValue::Scalar`
    /// (JSON-encoded, same shape as the existing `get`), nested `Y.Map` /
    /// `Y.Array` / `Y.Text` come back as `YrsValue::YMap` / `YArray` / `YText`
    /// carrying an `Arc` handle that observes/mutates the same branch.
    ///
    /// Returns `None` if the key is absent (matching `Y.Map.get` returning
    /// `undefined`); never errors. Internal helper — the FFI surface uses the
    /// `value_kind_at_key` + `get_<kind>_at_key` family below.
    pub(crate) fn get_value(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<YrsValue> {
        let binding = transaction.transaction();
        let tx = binding.as_ref().unwrap();
        let map = self.0.borrow();
        map.get(tx, key.as_str()).map(YrsValue::from_yrs_value)
    }

    /// FFI: returns the discriminator of the value at `key`, or `None` if the
    /// key is absent. Cheap — used by Swift to choose which `get_*_at_key` to
    /// call.
    pub(crate) fn value_kind_at_key(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<YrsValueKind> {
        self.get_value(transaction, key).map(|v| v.kind())
    }

    /// FFI: scalar accessor. Returns the JSON-encoded scalar at `key`, or
    /// `None` if the key is absent OR the value is a nested CRDT type. The
    /// existing `get(key)` method (which throws on non-scalar) stays in place
    /// for backward-compat with current Swift callers; this one is the new
    /// non-throwing variant that pairs with the kind discriminator.
    pub(crate) fn get_scalar_at_key(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<String> {
        match self.get_value(transaction, key)? {
            YrsValue::Scalar { json } => Some(json),
            _ => None,
        }
    }

    /// FFI: returns a live handle to the nested `Y.Map` at `key`, or `None`
    /// if the key is absent / the value is not a `Y.Map`.
    pub(crate) fn get_map_at_key(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<Arc<YrsMap>> {
        match self.get_value(transaction, key)? {
            YrsValue::YMap { value } => Some(value),
            _ => None,
        }
    }

    /// FFI: returns a live handle to the nested `Y.Array` at `key`, or `None`.
    pub(crate) fn get_array_at_key(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<Arc<YrsArray>> {
        match self.get_value(transaction, key)? {
            YrsValue::YArray { value } => Some(value),
            _ => None,
        }
    }

    /// FFI: returns a live handle to the nested `Y.Text` at `key`, or `None`.
    pub(crate) fn get_text_at_key(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Option<Arc<YrsText>> {
        match self.get_value(transaction, key)? {
            YrsValue::YText { value } => Some(value),
            _ => None,
        }
    }

    /// Inserts an empty nested `Y.Map` at `key` and returns a live handle to it.
    /// Mirrors JS `parentMap.set(key, new Y.Map())` followed by
    /// `parentMap.get(key)` — except we hand the handle back atomically so
    /// callers can immediately seed the nested map inside the same transaction.
    pub(crate) fn insert_map(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Arc<YrsMap> {
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted: MapRef = parent.insert(tx, key, MapPrelim::<Any>::new());
        Arc::new(YrsMap::from(inserted))
    }

    /// Inserts an empty nested `Y.Array` at `key` and returns a live handle.
    pub(crate) fn insert_array(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Arc<YrsArray> {
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted = parent.insert(tx, key, ArrayPrelim::<[Any; 0], Any>::from([]));
        Arc::new(YrsArray::from(inserted))
    }

    /// Inserts an empty nested `Y.Text` at `key` and returns a live handle.
    pub(crate) fn insert_text(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Arc<YrsText> {
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();
        let parent = self.0.borrow_mut();
        let inserted = parent.insert(tx, key, TextPrelim::new(""));
        Arc::new(YrsText::from(inserted))
    }

    pub(crate) fn remove(
        &self,
        transaction: &YrsTransaction,
        key: String,
    ) -> Result<Option<String>, CodingError> {
        // acquire a *mutable* transaction
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();

        // get a mutable reference to the YrsMap this type wraps
        let map = self.0.borrow_mut();

        let optional_value = map.remove(tx, key.as_str());
        match optional_value {
            // there was some kind of value in the map, try to cast it and convert
            // to JSON
            Some(v) => {
                if let Value::Any(any) = v {
                    let mut buf = String::new();
                    any.to_json(&mut buf);
                    return Ok(Some(buf));
                } else {
                    return Err(CodingError::EncodingError);
                }
            }
            // No value returned from the map on remove, so return the Optional
            // string as None.
            None => {
                return Ok(None);
            }
        }
    }

    pub(crate) fn clear(&self, transaction: &YrsTransaction) {
        // acquire a *mutable* transaction
        let mut binding = transaction.transaction();
        let tx = binding.as_mut().unwrap();

        // get a mutable reference to the YrsMap this type wraps
        let map = self.0.borrow_mut();

        map.clear(tx);
    }

    pub(crate) fn keys(
        &self,
        transaction: &YrsTransaction,
        delegate: Box<dyn YrsMapIteratorDelegate>,
    ) {
        // The internal `keys` function in Rust returns an explicit iterator that you can
        // fiddle with.
        //
        // fn keys<'a, T: ReadTxn + 'a>(&'a self, txn: &'a T) -> Keys<'a, &'a T, T>
        //
        // For these language bindings we're instead holding onto the iterator
        // ourselves, and expecting a delegate type from the language binding side that
        // we call with each value as it is available.

        // get a mutable transaction
        let binding = transaction.transaction();
        let txn = binding.as_ref().unwrap();

        let map = self.0.borrow();
        map.keys(txn).for_each(|key_value| {
            delegate.call(key_value.to_string());
        });
    }

    pub(crate) fn values(
        &self,
        transaction: &YrsTransaction,
        delegate: Box<dyn YrsMapIteratorDelegate>,
    ) {
        // Like the `keys` iterator pattern, we're holding onto the Rust iterator
        // ourselves, and expecting a delegate type from the language binding side that
        // we call with each value as it is available.

        // get a mutable transaction
        let binding = transaction.transaction();
        let txn = binding.as_ref().unwrap();

        let map = self.0.borrow();
        let iterator = map.values(txn);
        iterator.for_each(|value_list| {
            // value is being returned as Vec<Value> from YrsMap - unclear
            // why, but maybe we iterate over each element and attempt to any.to_json on it?
            // 20mar2023 - checking w/ Bartosz on if I'm missing something about
            // the values iterator here.
            //
            // The upstream yrs value iterator goes into the Yrs internal type
            // `Item`, which can potentially contain a list of values within it.
            // In practice, it appears to contains a single value for this usage of it.
            value_list.iter().for_each(|val_in_list| {
                let mut buf = String::new();
                if let Value::Any(any) = val_in_list {
                    any.to_json(&mut buf);
                    delegate.call(buf);
                } else {
                    // @TODO: fix silly handling, it will just call with empty string if casting fails
                    delegate.call(buf);
                }
            });
        });
    }

    pub(crate) fn each(
        &self,
        transaction: &YrsTransaction,
        delegate: Box<dyn YrsMapKVIteratorDelegate>,
    ) {
        // Like the `keys` and `values` iterator pattern, we're holding onto the Rust iterator
        // ourselves, and expecting a delegate type from the language binding side that
        // we call with each value as it is available.

        // get a mutable transaction
        let binding = transaction.transaction();
        let txn = binding.as_ref().unwrap();

        let map = self.0.borrow();
        let iterator = map.iter(txn);
        iterator.for_each(|key_value_pair| {
            // key_value_pair is being returned as a tuple of (&str, Value)
            // we'll pass the key value (String) straight through to the delegate,
            // but do the extra work to convert Value to a JSON string for decoding
            // on the far side of the language binding - or at least try to.
            let mut buf = String::new();
            if let Value::Any(any) = key_value_pair.1 {
                any.to_json(&mut buf);
                delegate.call(key_value_pair.0.to_string(), buf);
            } else {
                // @TODO: fix silly handling, it will just call with empty string if casting fails
                delegate.call(key_value_pair.0.to_string(), buf);
            }
        });
    }

    pub(crate) fn observe(&self, delegate: Box<dyn YrsMapObservationDelegate>) -> Arc<YSubscription> {
        let subscription = self
            .0
            .borrow_mut()
            .observe(move |transaction, map_event| {
                let delta = map_event.keys(transaction);
                let result: Vec<YrsMapChange> = delta
                    .iter()
                    .map(|val| YrsMapChange {
                        key: val.0.to_string(),
                        change: YrsEntryChange::from(val.1),
                    })
                    .collect();
                delegate.call(result)
            });

            Arc::new(YSubscription::new(subscription))
    }
}

#[cfg(test)]
mod tests {
    use crate::value::YrsValue;
    use crate::YrsDoc;

    #[test]
    fn map_insert_nested_map_returns_handle_and_get_value_round_trips() {
        let doc = YrsDoc::new();
        let plans = doc.get_map("plans".to_string());
        let txn = doc.transact(None);

        let plan = plans.insert_map(&txn, "2025-18".to_string());
        // Nested handle should be live and writable.
        plan.insert(&txn, "rerollCount".to_string(), "3".to_string());

        // Pulling the same key back out should give us a YMap variant whose
        // contents reflect the writes through the original handle. Note: the
        // existing `Any::to_json` round-trip on the scalar `get` path normalises
        // integers to "3.0" — see `lib/src/map.rs::get`. Nested-traversal does
        // not change that contract.
        let pulled = plans.get_value(&txn, "2025-18".to_string()).unwrap();
        match pulled {
            YrsValue::YMap { value } => {
                assert_eq!(value.length(&txn), 1);
                assert_eq!(
                    value.get(&txn, "rerollCount".to_string()).unwrap(),
                    "3.0".to_string()
                );
            }
            other => panic!("expected YMap variant, got {:?}", other),
        }
    }

    #[test]
    fn map_insert_nested_array_returns_handle_and_get_value_round_trips() {
        let doc = YrsDoc::new();
        let plans = doc.get_map("plans".to_string());
        let txn = doc.transact(None);

        let plan = plans.insert_map(&txn, "2025-18".to_string());
        let recipe_ids = plan.insert_array(&txn, "recipeIds".to_string());
        recipe_ids.insert(&txn, 0, "\"rec_abc\"".to_string());
        recipe_ids.insert(&txn, 1, "\"rec_def\"".to_string());

        let pulled_plan = plans.get_value(&txn, "2025-18".to_string()).unwrap();
        let plan_handle = match pulled_plan {
            YrsValue::YMap { value } => value,
            other => panic!("expected YMap, got {:?}", other),
        };
        let pulled_arr = plan_handle
            .get_value(&txn, "recipeIds".to_string())
            .unwrap();
        match pulled_arr {
            YrsValue::YArray { value } => {
                assert_eq!(value.length(&txn), 2);
                assert_eq!(value.to_a(&txn), vec!["\"rec_abc\"", "\"rec_def\""]);
            }
            other => panic!("expected YArray, got {:?}", other),
        }
    }

    #[test]
    fn map_get_value_returns_scalar_for_primitive() {
        let doc = YrsDoc::new();
        let map = doc.get_map("recipes".to_string());
        let txn = doc.transact(None);
        map.insert(&txn, "title".to_string(), "\"Pasta\"".to_string());
        let v = map.get_value(&txn, "title".to_string()).unwrap();
        match v {
            YrsValue::Scalar { json } => assert_eq!(json, "\"Pasta\""),
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn map_insert_text_returns_handle() {
        let doc = YrsDoc::new();
        let map = doc.get_map("notes".to_string());
        let txn = doc.transact(None);
        let text = map.insert_text(&txn, "body".to_string());
        text.append(&txn, "hello".to_string());
        let pulled = map.get_value(&txn, "body".to_string()).unwrap();
        match pulled {
            YrsValue::YText { value } => assert_eq!(value.get_string(&txn), "hello"),
            other => panic!("expected YText, got {:?}", other),
        }
    }

    #[test]
    fn map_get_value_returns_none_for_missing_key() {
        let doc = YrsDoc::new();
        let map = doc.get_map("recipes".to_string());
        let txn = doc.transact(None);
        assert!(map.get_value(&txn, "missing".to_string()).is_none());
    }

    #[test]
    fn verify_new_map_has_zero_count() {
        let doc = YrsDoc::new();
        let map = doc.get_map("example_map".to_string());

        let txn = doc.transact(None);
        assert_eq!(map.length(&txn), 0);
    }

    #[test]
    fn map_insert_and_count() {
        let doc = YrsDoc::new();
        let map = doc.get_map("example_map".to_string());

        let key_to_insert = "AB123".to_string();
        let value_to_insert = "\"Hello\"".to_string();

        let txn = doc.transact(None);

        assert_eq!(map.contains_key(&txn, key_to_insert.clone()), false);

        map.insert(&txn, key_to_insert.clone(), value_to_insert);
        assert_eq!(map.length(&txn), 1);

        assert_eq!(map.contains_key(&txn, key_to_insert), true);
    }

    #[test]
    fn map_insert_and_get() {
        let doc = YrsDoc::new();
        let map = doc.get_map("example_map".to_string());

        let key_to_insert = "AB123".to_string();
        let value_to_insert = "\"Hello\"".to_string();

        let txn = doc.transact(None);

        assert_eq!(map.contains_key(&txn, key_to_insert.clone()), false);

        map.insert(&txn, key_to_insert.clone(), value_to_insert.clone());
        assert_eq!(map.length(&txn), 1);

        let result = map.get(&txn, key_to_insert.clone()).unwrap();
        assert_eq!(result, value_to_insert);
    }

    #[test]
    fn map_remove() {
        let doc = YrsDoc::new();
        let map = doc.get_map("example_map".to_string());

        let key_to_insert = "AB123".to_string();
        let value_to_insert = "\"Hello\"".to_string();

        let txn = doc.transact(None);

        assert_eq!(map.contains_key(&txn, key_to_insert.clone()), false);

        map.insert(&txn, key_to_insert.clone(), value_to_insert.clone());

        let returned = map.remove(&txn, key_to_insert.clone());
        let unwrapped_return = returned.unwrap();
        assert_eq!(unwrapped_return, Some(value_to_insert.clone()));
        assert_eq!(map.length(&txn), 0);
    }

    #[test]
    fn map_clear() {
        let doc = YrsDoc::new();
        let map = doc.get_map("example_map".to_string());

        let key_to_insert = "AB123".to_string();
        let value_to_insert = "\"Hello\"".to_string();

        let txn = doc.transact(None);

        map.insert(&txn, key_to_insert.clone(), value_to_insert.clone());
        assert_eq!(map.length(&txn), 1);

        map.clear(&txn);
        assert_eq!(map.length(&txn), 0);
    }

    /*
        ## The section below is Joe trying to sort out the pieces to make a unit test
        that "works" the code structure when you invoke "keys" - which involves multiple
        calls to a delegate object that you need to provide. I haven't been able to figure
        out how to structure the dyn Box<T> object and get it implementing the required
        trait on the Rust side of things: `crate::map::YrsMapIteratorDelegate`

        I'll work/test the pattern through the Swift language side of this binding setup,
        but I'd really like to understand how to get it working on the Rust side as well.
        For now, however, I'll just leave this at where I got to - and hope to come back to
        resolve it in the future with some more experience Rust brains alongside.

        #[derive(Debug)]
        struct KeyDelegate {
            collected: Vec<String>
        }
        // Marks that this type can be transferred across thread boundaries.
        //unsafe impl Send for RefCell<KeyDelegate> {}
        // Marks that this type is safe to share references between threads.
        unsafe impl Sync for KeyDelegate {}

        impl KeyDelegate {

            fn append(&mut self, value: String) {
                &self.collected.push(value);
            }

            fn new() -> KeyDelegate {
                let newDelegate = KeyDelegate {
                    collected: Vec::<String>::new()
                };
                return newDelegate
            }

            // fn test(&self) -> Box<dyn crate::map::YrsMapIteratorDelegate> {
            //     return Box::new(self)
            // }
        }

        impl crate::map::YrsMapIteratorDelegate for Box<KeyDelegate> {
            fn call(&self, key_value: String) {
                self.append(key_value)
            }
        }

        // impl crate::map::YrsMapIteratorDelegate for KeyDelegate {
        //     fn call(&self, key_value: String) {

        //     }
        // }

        #[test]
        fn map_keys() {
            let doc = YrsDoc::new();
            let map = doc.get_map("example_map".to_string());

            let first_key_to_insert = "AB123".to_string();
            let second_key_to_insert = "890YZ".to_string();
            let value_to_insert = "\"Hello\"".to_string();

            let txn = doc.transact();

            map.insert(&txn, first_key_to_insert.clone(), value_to_insert.clone());
            map.insert(&txn, second_key_to_insert.clone(), value_to_insert.clone());
            assert_eq!(map.length(&txn), 2);

            let delegate = Box::new(KeyDelegate::new());
            map.keys(&txn, delegate);
    //                     ^^^^^^^^ the trait `YrsMapIteratorDelegate` is not implemented for `KeyDelegate`
    //                     Compiler error when invoking `cargo test`
            assert_eq!(delegate.collected.len(), 2);
        }

     */
}

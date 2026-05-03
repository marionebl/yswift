import Foundation
import Yniffi

// MARK: - Nested-traversal extensions on the FFI types
//
// uniffi exposes the new Rust API as four sibling getters on `YrsMap` /
// `YrsArray` (`valueKindAtKey`, `getScalarAtKey`, `getMapAtKey`,
// `getArrayAtKey`, `getTextAtKey` and the equivalent `*AtIndex` family on
// `YrsArray`). Calling those individually is awkward; the extensions below
// recompose them into a single `value(forKey:transaction:)` returning a
// `YValue` sum type, plus typed convenience accessors that match the JS
// `Y.Map.get(key)?` / `Y.Array.get(i)?` shape one-to-one.
//
// All methods take the caller's `YrsTransaction` directly. Get one via
// `YDocument.transactSync { txn in … }` — the same transaction handle the
// rest of YSwift uses.

public extension YrsMap {
    /// Returns the typed value at `key`, or `nil` if the key is absent.
    ///
    /// Mirrors JS `Y.Map.get(key)`: scalars come back as `.scalar(json:)`,
    /// nested CRDT types come back as `.map`, `.array`, `.text` carrying a
    /// live handle.
    func value(forKey key: String, transaction: YrsTransaction) -> YValue? {
        guard let kind = valueKindAtKey(tx: transaction, key: key) else { return nil }
        switch kind {
        case .scalar:
            return getScalarAtKey(tx: transaction, key: key).map { .scalar(json: $0) }
        case .yMap:
            return getMapAtKey(tx: transaction, key: key).map { .map($0) }
        case .yArray:
            return getArrayAtKey(tx: transaction, key: key).map { .array($0) }
        case .yText:
            return getTextAtKey(tx: transaction, key: key).map { .text($0) }
        }
    }

    /// Returns the nested `Y.Map` handle at `key`, or `nil` if absent / not a map.
    func nestedMap(forKey key: String, transaction: YrsTransaction) -> YrsMap? {
        getMapAtKey(tx: transaction, key: key)
    }

    /// Returns the nested `Y.Array` handle at `key`, or `nil` if absent / not an array.
    func nestedArray(forKey key: String, transaction: YrsTransaction) -> YrsArray? {
        getArrayAtKey(tx: transaction, key: key)
    }

    /// Returns the nested `Y.Text` handle at `key`, or `nil` if absent / not a text.
    func nestedText(forKey key: String, transaction: YrsTransaction) -> YrsText? {
        getTextAtKey(tx: transaction, key: key)
    }

    /// Decodes the scalar value at `key` into the requested `Codable` type.
    /// Returns `nil` if the key is missing, the value isn't a scalar, or the
    /// JSON doesn't decode into `T`.
    func scalar<T: Decodable>(forKey key: String, as: T.Type = T.self, transaction: YrsTransaction) -> T? {
        guard let json = getScalarAtKey(tx: transaction, key: key) else { return nil }
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    /// Inserts an empty nested `Y.Map` at `key` and returns a live handle to
    /// it. Callers can keep mutating the nested map within the same
    /// transaction.
    @discardableResult
    func insertNestedMap(forKey key: String, transaction: YrsTransaction) -> YrsMap {
        insertMap(tx: transaction, key: key)
    }

    /// Inserts an empty nested `Y.Array` at `key` and returns a live handle.
    @discardableResult
    func insertNestedArray(forKey key: String, transaction: YrsTransaction) -> YrsArray {
        insertArray(tx: transaction, key: key)
    }

    /// Inserts an empty nested `Y.Text` at `key` and returns a live handle.
    @discardableResult
    func insertNestedText(forKey key: String, transaction: YrsTransaction) -> YrsText {
        insertText(tx: transaction, key: key)
    }
}

public extension YrsArray {
    /// Returns the typed value at `index`, or `nil` if out of range.
    func value(at index: UInt32, transaction: YrsTransaction) -> YValue? {
        guard let kind = valueKindAtIndex(tx: transaction, index: index) else { return nil }
        switch kind {
        case .scalar:
            return getScalarAtIndex(tx: transaction, index: index).map { .scalar(json: $0) }
        case .yMap:
            return getMapAtIndex(tx: transaction, index: index).map { .map($0) }
        case .yArray:
            return getArrayAtIndex(tx: transaction, index: index).map { .array($0) }
        case .yText:
            return getTextAtIndex(tx: transaction, index: index).map { .text($0) }
        }
    }

    func nestedMap(at index: UInt32, transaction: YrsTransaction) -> YrsMap? {
        getMapAtIndex(tx: transaction, index: index)
    }

    func nestedArray(at index: UInt32, transaction: YrsTransaction) -> YrsArray? {
        getArrayAtIndex(tx: transaction, index: index)
    }

    func nestedText(at index: UInt32, transaction: YrsTransaction) -> YrsText? {
        getTextAtIndex(tx: transaction, index: index)
    }

    func scalar<T: Decodable>(at index: UInt32, as: T.Type = T.self, transaction: YrsTransaction) -> T? {
        guard let json = getScalarAtIndex(tx: transaction, index: index) else { return nil }
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    @discardableResult
    func insertNestedMap(at index: UInt32, transaction: YrsTransaction) -> YrsMap {
        insertMap(tx: transaction, index: index)
    }

    @discardableResult
    func insertNestedArray(at index: UInt32, transaction: YrsTransaction) -> YrsArray {
        insertArray(tx: transaction, index: index)
    }

    @discardableResult
    func insertNestedText(at index: UInt32, transaction: YrsTransaction) -> YrsText {
        insertText(tx: transaction, index: index)
    }
}

// MARK: - Convenience: untyped doc-root access
//
// The high-level `YMap<T>` / `YArray<T>` Swift wrappers impose a homogeneous
// `Codable T`, which doesn't fit a doc whose top-level Map values are
// themselves nested `Y.Map`s (preperoni's `recipes` and `plans` schemas).
// The two helpers below give callers direct access to the underlying
// `YrsMap` / `YrsArray` so they can use the nested-traversal API above
// without first unwrapping a generic type.

public extension YDocument {
    /// Returns the underlying `YrsMap` for a top-level map named `name`.
    /// Use this when the map's values are heterogeneous (nested CRDT types
    /// in particular) — `getOrCreateMap(named:)` only fits homogeneous
    /// `Codable` values.
    func nestedMap(named name: String) -> YrsMap {
        // YDocument's internal YrsDoc handle is private. Rather than reach in
        // we go via the existing typed factory and pull the handle off the
        // returned wrapper — they share the same underlying ref-counted
        // YrsMap so this is free.
        let typed: YMap<EmptyCodable> = getOrCreateMap(named: name)
        return typed.rawMap
    }

    /// Returns the underlying `YrsArray` for a top-level array named `name`.
    func nestedArray(named name: String) -> YrsArray {
        let typed: YArray<EmptyCodable> = getOrCreateArray(named: name)
        return typed.rawArray
    }
}

/// Sentinel `Codable` placeholder used by the doc-level nested accessors.
/// Never instantiated; only used to satisfy the `T: Codable` bound on the
/// homogeneous wrappers.
public struct EmptyCodable: Codable {}

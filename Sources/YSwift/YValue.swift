import Foundation
import Yniffi

/// A typed value pulled out of a `YMap` or `YArray`.
///
/// Mirrors the JS `Y.Map.get(key)` return shape: a primitive (encoded as a
/// `Codable` `T`), or a live handle to a nested `Y.Map<U>` / `Y.Array<U>` /
/// `Y.Text`. The handle observes and mutates the same CRDT branch as the
/// parent — reading a nested value never copies.
///
/// uniffi's UDL backend forbids object-typed payloads inside generated enum
/// variants, so the FFI layer surfaces the nested-traversal API as four
/// sibling getters (`getMapAtKey`, `getArrayAtKey`, `getTextAtKey`,
/// `getScalarAtKey`) plus a discriminator (`valueKindAtKey`). This Swift
/// enum recomposes them into a single sum type that callers can `switch`
/// over.
public enum YValue {
    /// A JSON-encoded primitive — string / number / bool / null / JSON array
    /// or object. Decode with `decode(_:)` to recover the underlying Swift
    /// `Codable` type.
    case scalar(json: String)

    /// A live handle to a nested `Y.Map`. The element type is determined by
    /// the caller — Y itself is dynamically typed, so we cannot infer it
    /// from the wire.
    case map(YrsMap)

    /// A live handle to a nested `Y.Array`.
    case array(YrsArray)

    /// A live handle to a nested `Y.Text`.
    case text(YrsText)
}

public extension YValue {
    /// Decode the scalar payload into the requested `Codable` type.
    ///
    /// Returns `nil` if this value is not a scalar, or if the JSON does not
    /// decode into `T`. Mirrors the JS pattern
    /// `if (typeof v === "string") JSON.parse(v) as MyType`.
    func decode<T: Decodable>(_ type: T.Type = T.self) -> T? {
        guard case let .scalar(json) = self else { return nil }
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    /// Pull the nested map handle out, or `nil` if this is not a map.
    var asMap: YrsMap? {
        if case let .map(m) = self { return m }
        return nil
    }

    /// Pull the nested array handle out, or `nil` if this is not an array.
    var asArray: YrsArray? {
        if case let .array(a) = self { return a }
        return nil
    }

    /// Pull the nested text handle out, or `nil` if this is not a text.
    var asText: YrsText? {
        if case let .text(t) = self { return t }
        return nil
    }
}

import XCTest
@testable import YSwift

/// Coverage for the nested-collection traversal API added in
/// `YNestedAccess.swift` / `value.rs`. Mirrors the Rust unit tests in
/// `lib/src/map.rs` / `lib/src/array.rs` from the Swift side, ensuring the
/// FFI surface (`valueKindAtKey`, `getMapAtKey`, `insertMap`, …) is wired
/// through correctly and that the Swift sugar (`YValue` enum + extensions)
/// behaves like JS `Y.Map.get(key)`.
final class YNestedAccessTests: XCTestCase {
    var document: YDocument!

    override func setUp() {
        document = YDocument()
    }

    override func tearDown() {
        document = nil
    }

    // MARK: - Map → nested children

    func test_insertNestedMap_returnsLiveHandle() {
        let plans = document.nestedMap(named: "plans")
        document.transactSync { txn in
            let plan = plans.insertNestedMap(forKey: "2025-18", transaction: txn)
            plan.insert(tx: txn, key: "rerollCount", value: "3")

            // Pulling the same key back out gives a YValue.map carrying the
            // *same* live handle — writes through it must be visible.
            guard case let .map(pulled) = plans.value(forKey: "2025-18", transaction: txn) else {
                return XCTFail("expected .map")
            }
            XCTAssertEqual(pulled.length(tx: txn), 1)
            XCTAssertEqual(try? pulled.get(tx: txn, key: "rerollCount"), "3.0")
        }
    }

    func test_insertNestedArray_acceptsStringElementsAndRoundTrips() {
        let plans = document.nestedMap(named: "plans")
        document.transactSync { txn in
            let plan = plans.insertNestedMap(forKey: "2025-18", transaction: txn)
            let ids = plan.insertNestedArray(forKey: "recipeIds", transaction: txn)
            ids.insert(tx: txn, index: 0, value: "\"rec_abc\"")
            ids.insert(tx: txn, index: 1, value: "\"rec_def\"")

            let pulled = plan.nestedArray(forKey: "recipeIds", transaction: txn)
            XCTAssertEqual(pulled?.length(tx: txn), 2)
            XCTAssertEqual(pulled?.toA(tx: txn), ["\"rec_abc\"", "\"rec_def\""])
        }
    }

    func test_value_returnsScalarForPrimitives() {
        let m = document.nestedMap(named: "recipes")
        document.transactSync { txn in
            m.insert(tx: txn, key: "title", value: "\"Pasta\"")
            guard case let .scalar(json) = m.value(forKey: "title", transaction: txn) else {
                return XCTFail("expected .scalar")
            }
            XCTAssertEqual(json, "\"Pasta\"")
        }
    }

    func test_value_returnsNilForMissingKey() {
        let m = document.nestedMap(named: "recipes")
        document.transactSync { txn in
            XCTAssertNil(m.value(forKey: "missing", transaction: txn))
        }
    }

    func test_scalarDecode_returnsTypedSwiftValue() {
        let m = document.nestedMap(named: "recipes")
        document.transactSync { txn in
            m.insert(tx: txn, key: "servings", value: "4")
            // Note: the FFI scalar layer normalises numbers via Any::to_json
            // ("4" -> "4.0"), so we decode through Double; see Rust tests.
            let v: Double? = m.scalar(forKey: "servings", transaction: txn)
            XCTAssertEqual(v, 4.0)
        }
    }

    func test_nestedTextHandleIsLive() {
        let m = document.nestedMap(named: "notes")
        document.transactSync { txn in
            let t = m.insertNestedText(forKey: "body", transaction: txn)
            t.append(tx: txn, text: "hello")
            let pulled = m.nestedText(forKey: "body", transaction: txn)
            XCTAssertEqual(pulled?.getString(tx: txn), "hello")
        }
    }

    // MARK: - Array → nested children

    func test_array_insertNestedMap_isLive() {
        let arr = document.nestedArray(named: "rows")
        document.transactSync { txn in
            let row = arr.insertNestedMap(at: 0, transaction: txn)
            row.insert(tx: txn, key: "name", value: "\"Pasta\"")

            guard case let .map(pulled) = arr.value(at: 0, transaction: txn) else {
                return XCTFail("expected .map")
            }
            XCTAssertEqual(pulled.length(tx: txn), 1)
        }
    }

    func test_array_valueOutOfRangeIsNil() {
        let arr = document.nestedArray(named: "xs")
        document.transactSync { txn in
            XCTAssertNil(arr.value(at: 0, transaction: txn))
        }
    }

    // MARK: - Cross-document round-trip via apply_update

    /// Encodes the full preperoni-shaped doc on one side, applies the update
    /// on a second YDocument, and walks the nested types using the new API.
    /// Proves the wire format survives `encodeStateAsUpdate` / `applyUpdate`
    /// — the same path SSE uses on the iOS app.
    func test_applyUpdate_preservesNestedStructure() {
        let producer = YDocument()
        let pRecipes = producer.nestedMap(named: "recipes")
        let pPlans = producer.nestedMap(named: "plans")

        let update: [UInt8] = producer.transactSync { txn in
            let rec = pRecipes.insertNestedMap(forKey: "rec_abc", transaction: txn)
            rec.insert(tx: txn, key: "title", value: "\"Pasta\"")

            let plan = pPlans.insertNestedMap(forKey: "2025-18", transaction: txn)
            let ids = plan.insertNestedArray(forKey: "recipeIds", transaction: txn)
            ids.insert(tx: txn, index: 0, value: "\"rec_abc\"")

            return txn.transactionEncodeStateAsUpdate()
        }

        let consumer = YDocument()
        let cRecipes = consumer.nestedMap(named: "recipes")
        let cPlans = consumer.nestedMap(named: "plans")
        consumer.transactSync { txn in
            try! txn.transactionApplyUpdate(update: update)
        }

        consumer.transactSync { txn in
            let rec = cRecipes.nestedMap(forKey: "rec_abc", transaction: txn)
            XCTAssertEqual(try? rec?.get(tx: txn, key: "title"), "\"Pasta\"")

            let plan = cPlans.nestedMap(forKey: "2025-18", transaction: txn)
            let ids = plan?.nestedArray(forKey: "recipeIds", transaction: txn)
            XCTAssertEqual(ids?.toA(tx: txn), ["\"rec_abc\""])
        }
    }
}

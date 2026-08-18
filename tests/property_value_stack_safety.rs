//! Public-API integration gates for flat, stack-safe property values.

use libcpg::{PropertyValue, PropertyValueKind};

const DEPTH: usize = 100_000;

fn deep_unary_value() -> PropertyValue {
    let mut value = PropertyValue::Null;
    for _ in 0..DEPTH {
        value = PropertyValue::List(vec![value]);
    }
    value
}

fn assert_unary_depth(value: &PropertyValue) {
    let mut cursor = value.as_ref();
    let mut depth = 0usize;
    loop {
        match cursor.kind() {
            PropertyValueKind::List(children) => {
                assert_eq!(children.len(), 1);
                cursor = children.get(0).expect("unary child");
                depth += 1;
            }
            PropertyValueKind::Null => break,
            _ => panic!("deep unary fixture changed logical shape"),
        }
    }
    assert_eq!(depth, DEPTH);
}

#[test]
fn public_lifecycle_and_codec_handle_depth_100k_on_a_small_stack() {
    let handle = std::thread::Builder::new()
        .name("property-value-lifecycle-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let value = deep_unary_value();
            assert_eq!(value.metrics().nodes, DEPTH + 1);
            assert_eq!(value.metrics().edges, DEPTH);
            assert_unary_depth(&value);

            let cloned = value.clone();
            assert!(cloned == value);
            let debug = format!("{value:?}");
            assert!(debug.starts_with("List([List(["));
            assert!(debug.contains("Null"));
            assert_eq!(debug.matches("List([").count(), DEPTH);
            assert_eq!(debug.matches("])").count(), DEPTH);

            let bytes = value.encode().expect("encode depth-100k property");
            let decoded = PropertyValue::decode(&bytes).expect("decode depth-100k property");
            assert!(decoded == value);
            assert_unary_depth(&decoded);

            drop(debug);
            drop(bytes);
            drop(decoded);
            drop(cloned);
            drop(value);
        })
        .expect("spawn property-value small-stack thread");

    handle
        .join()
        .expect("flat property lifecycle completes on a small native stack");
}

#[cfg(feature = "serde")]
#[test]
fn public_serde_path_handles_depth_100k_on_a_small_stack() {
    let handle = std::thread::Builder::new()
        .name("property-value-serde-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let value = deep_unary_value();
            let json = serde_json::to_vec(&value).expect("serialize deep property through serde");
            let decoded: PropertyValue =
                serde_json::from_slice(&json).expect("deserialize deep property through serde");
            assert!(decoded == value);
            assert_unary_depth(&decoded);
            drop(decoded);
            drop(json);
            drop(value);
        })
        .expect("spawn property-value serde small-stack thread");

    handle
        .join()
        .expect("serde property lifecycle completes on a small native stack");
}

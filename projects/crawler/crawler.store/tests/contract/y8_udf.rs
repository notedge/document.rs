//! Y-8 版本化纯 UDF：同输入同输出，旧版本不受新版本注册影响。

use yydb::{Connection, Error, Value};

#[test]
fn y8_versioned_pure_udf_probe() {
    let conn = Connection::open_in_memory().expect("open in memory");
    conn.create_scalar_versioned("crawler.plus_one", 1, 1, |args| match args.first() {
        Some(Value::I64(value)) => Ok(Value::I64(value + 1)),
        _ => Err(Error::Udf { name: "crawler.plus_one".into(), message: "expected i64".into() }),
    })
    .expect("register v1");
    conn.create_scalar_versioned("crawler.plus_one", 2, 1, |args| match args.first() {
        Some(Value::I64(value)) => Ok(Value::I64(value + 2)),
        _ => Err(Error::Udf { name: "crawler.plus_one".into(), message: "expected i64".into() }),
    })
    .expect("register v2");

    let input = [Value::I64(41)];
    let first = conn.call_scalar_version("crawler.plus_one", 1, &input).expect("call v1");
    let second = conn.call_scalar_version("crawler.plus_one", 1, &input).expect("call v1 again");
    assert_eq!(first, second, "pure UDF must be deterministic");
    assert_eq!(first, Value::I64(42), "v2 registration must not change v1");
    assert_eq!(conn.call_scalar_version("crawler.plus_one", 2, &input).expect("call v2"), Value::I64(43));
    assert!(
        matches!(conn.call_scalar_version("crawler.plus_one", 3, &input), Err(Error::UdfVersionMismatch { .. })),
        "unknown UDF version must fail closed"
    );
}

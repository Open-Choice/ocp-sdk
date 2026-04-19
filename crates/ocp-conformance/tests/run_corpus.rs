//! Runs the bundled fixture corpus against `ocp-types-v1`.
//!
//! Each fixture is dispatched to the appropriate `ocp-types-v1` parser based
//! on its `target` field, then checked against the expected `assertion`.
//!
//! When the spec and the corpus disagree, the corpus wins. If a test here
//! fails, the bug is either in the spec, the implementation, or the fixture
//! — investigate which before "fixing" the test.

use std::collections::BTreeSet;

use ocp_conformance::{
    bundled_fixtures_dir, load_corpus, Fixture, FixtureAssertion, FixtureTarget,
};
use ocp_types_v1::{
    capabilities::{validate_capability_set, Capability},
    envelope::Envelope,
    kind::Kind,
    manifest::Manifest,
    wire::{ContentDigest, Duration, Identifier, PathRef},
};
use serde_json::Value;

fn run_fixture(category: &str, fixture: &Fixture) -> Result<(), String> {
    match (fixture.target, fixture.assertion) {
        // ----- Envelope -----
        (FixtureTarget::Envelope, FixtureAssertion::Roundtrip) => {
            roundtrip::<Envelope>(&fixture.input)
        }
        (FixtureTarget::Envelope, FixtureAssertion::PreserveUnknown) => {
            preserve_unknown::<Envelope>(&fixture.input, &fixture.expect_preserved)
        }
        (FixtureTarget::Envelope, FixtureAssertion::Reject) => reject::<Envelope>(&fixture.input),

        // ----- Kind -----
        (FixtureTarget::Kind, FixtureAssertion::Roundtrip) => {
            let s = fixture
                .input
                .as_str()
                .ok_or_else(|| "kind fixture input must be a string".to_string())?;
            Kind::parse(s).map(|_| ()).map_err(|e| e.to_string())
        }
        (FixtureTarget::Kind, FixtureAssertion::Reject) => {
            let s = fixture
                .input
                .as_str()
                .ok_or_else(|| "kind fixture input must be a string".to_string())?;
            match Kind::parse(s) {
                Ok(_) => Err(format!("expected kind '{}' to be rejected, but it parsed", s)),
                Err(_) => Ok(()),
            }
        }

        // ----- Capability (single) -----
        (FixtureTarget::Capability, FixtureAssertion::Roundtrip) => {
            let s = fixture
                .input
                .as_str()
                .ok_or_else(|| "capability fixture input must be a string".to_string())?;
            Capability::parse(s).map(|_| ()).map_err(|e| e.to_string())
        }
        (FixtureTarget::Capability, FixtureAssertion::Reject) => {
            let s = fixture
                .input
                .as_str()
                .ok_or_else(|| "capability fixture input must be a string".to_string())?;
            match Capability::parse(s) {
                Ok(_) => Err(format!(
                    "expected capability '{}' to be rejected, but it parsed",
                    s
                )),
                Err(_) => Ok(()),
            }
        }

        // ----- Capability list (closure validation) -----
        (FixtureTarget::CapabilityList, FixtureAssertion::CapabilityClosureValid) => {
            let caps = parse_capability_list(&fixture.input)?;
            validate_capability_set(&caps).map_err(|e| e.to_string())
        }
        (FixtureTarget::CapabilityList, FixtureAssertion::CapabilityClosureInvalid) => {
            let caps = parse_capability_list(&fixture.input)?;
            match validate_capability_set(&caps) {
                Ok(_) => Err("expected capability set to fail closure, but it passed".to_string()),
                Err(_) => Ok(()),
            }
        }

        // ----- Manifest -----
        (FixtureTarget::Manifest, FixtureAssertion::Roundtrip) => {
            roundtrip::<Manifest>(&fixture.input)
        }
        (FixtureTarget::Manifest, FixtureAssertion::PreserveUnknown) => {
            preserve_unknown::<Manifest>(&fixture.input, &fixture.expect_preserved)
        }
        (FixtureTarget::Manifest, FixtureAssertion::Reject) => reject::<Manifest>(&fixture.input),

        // ----- Identifier -----
        (FixtureTarget::Identifier, FixtureAssertion::Roundtrip) => {
            roundtrip::<Identifier>(&fixture.input)
        }

        // ----- Duration -----
        (FixtureTarget::Duration, FixtureAssertion::Roundtrip) => {
            // Duration round-trip is asymmetric: omitted nanos serialize back as omitted.
            // We parse, then re-serialize, then parse again to verify the round-trip is stable.
            let parsed: Duration = serde_json::from_value(fixture.input.clone())
                .map_err(|e| format!("first parse: {}", e))?;
            let serialized = serde_json::to_value(parsed)
                .map_err(|e| format!("re-serialize: {}", e))?;
            let _: Duration = serde_json::from_value(serialized)
                .map_err(|e| format!("re-parse: {}", e))?;
            Ok(())
        }

        // ----- ContentDigest -----
        (FixtureTarget::ContentDigest, FixtureAssertion::Roundtrip) => {
            roundtrip::<ContentDigest>(&fixture.input)
        }

        // ----- PathRef -----
        (FixtureTarget::PathRef, FixtureAssertion::Roundtrip) => {
            roundtrip::<PathRef>(&fixture.input)
        }

        (target, assertion) => Err(format!(
            "[{}] fixture {}: unsupported (target={:?}, assertion={:?})",
            category, fixture.id, target, assertion
        )),
    }
}

/// Parse and re-serialize, asserting that the round-trip is lossless.
fn roundtrip<T>(input: &Value) -> Result<(), String>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let parsed: T = serde_json::from_value(input.clone())
        .map_err(|e| format!("parse failed: {}", e))?;
    let serialized = serde_json::to_value(&parsed)
        .map_err(|e| format!("serialize failed: {}", e))?;
    // For round-trip, every key in the input MUST be present in the output with
    // the same value. We don't require strict equality because optional fields
    // that are absent in the input may be reordered.
    assert_keys_preserved(input, &serialized)?;
    Ok(())
}

/// Parse, re-serialize, then assert that every named field survived.
fn preserve_unknown<T>(input: &Value, expect: &[String]) -> Result<(), String>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let parsed: T = serde_json::from_value(input.clone())
        .map_err(|e| format!("parse failed: {}", e))?;
    let serialized = serde_json::to_value(&parsed)
        .map_err(|e| format!("serialize failed: {}", e))?;
    for key in expect {
        let original = input
            .get(key)
            .ok_or_else(|| format!("expected_preserved key '{}' not in input", key))?;
        let after = serialized
            .get(key)
            .ok_or_else(|| format!("preserved key '{}' was DROPPED on round-trip", key))?;
        if original != after {
            return Err(format!(
                "preserved key '{}' changed: before={} after={}",
                key, original, after
            ));
        }
    }
    Ok(())
}

/// Assert that parsing fails.
fn reject<T>(input: &Value) -> Result<(), String>
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_value::<T>(input.clone()) {
        Ok(_) => Err("expected parse to fail, but it succeeded".to_string()),
        Err(_) => Ok(()),
    }
}

fn parse_capability_list(input: &Value) -> Result<Vec<Capability>, String> {
    let arr = input
        .as_array()
        .ok_or_else(|| "capability_list input must be a JSON array".to_string())?;
    let mut caps = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item
            .as_str()
            .ok_or_else(|| "capability_list items must be strings".to_string())?;
        caps.push(Capability::parse(s).map_err(|e| e.to_string())?);
    }
    Ok(caps)
}

/// Recursively assert every key in `input` is present in `output` with the
/// same value. This is the round-trip property: a conformant parser preserves
/// every field it sees.
fn assert_keys_preserved(input: &Value, output: &Value) -> Result<(), String> {
    match (input, output) {
        (Value::Object(in_map), Value::Object(out_map)) => {
            let in_keys: BTreeSet<&String> = in_map.keys().collect();
            let out_keys: BTreeSet<&String> = out_map.keys().collect();
            for key in &in_keys {
                if !out_keys.contains(*key) {
                    return Err(format!("key '{}' was dropped on round-trip", key));
                }
                assert_keys_preserved(&in_map[*key], &out_map[*key])?;
            }
            Ok(())
        }
        (Value::Array(in_arr), Value::Array(out_arr)) => {
            if in_arr.len() != out_arr.len() {
                return Err(format!(
                    "array length changed: before={} after={}",
                    in_arr.len(),
                    out_arr.len()
                ));
            }
            for (i, (a, b)) in in_arr.iter().zip(out_arr.iter()).enumerate() {
                assert_keys_preserved(a, b).map_err(|e| format!("[{}]: {}", i, e))?;
            }
            Ok(())
        }
        (a, b) => {
            if a != b {
                Err(format!("value changed: before={} after={}", a, b))
            } else {
                Ok(())
            }
        }
    }
}

#[test]
fn run_bundled_corpus() {
    let dir = bundled_fixtures_dir();
    let corpus = load_corpus(&dir).expect("failed to load corpus");
    assert!(
        !corpus.is_empty(),
        "bundled corpus is empty — fixtures missing?"
    );

    let mut failures = Vec::new();
    let mut total = 0usize;
    for (category, fixture) in corpus.all() {
        total += 1;
        if let Err(e) = run_fixture(category, fixture) {
            failures.push(format!("[{}/{}] {}: {}", category, fixture.id, fixture.description, e));
        }
    }

    println!("conformance: {} fixtures total", total);
    if !failures.is_empty() {
        eprintln!("{} fixture(s) FAILED:", failures.len());
        for f in &failures {
            eprintln!("  - {}", f);
        }
        panic!("conformance suite failed");
    }
}

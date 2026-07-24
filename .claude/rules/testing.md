# Rust Testing

## 1. Treat tests as executable specifications

- **MUST** write tests that clearly describe the behavior the code is expected
  to provide.
- **MUST** test observable outcomes rather than implementation details whenever
  possible.
- **SHOULD** include both successful cases and meaningful failure or boundary
  cases.
- **SHOULD** keep each test focused on one behavior.

A Rust test is an ordinary function annotated with `#[test]`:

```rust
#[test]
fn addition_works() {
    assert_eq!(2 + 2, 4);
}
```

## 2. Use the standard test structure

Each test **SHOULD** follow this sequence:

1. Arrange the required data or state.
2. Execute the code under test.
3. Assert that the result matches the expected behavior.

```rust
#[test]
fn rectangle_can_hold_smaller_rectangle() {
    let larger = Rectangle {
        width: 8,
        height: 7,
    };
    let smaller = Rectangle {
        width: 5,
        height: 1,
    };

    assert!(larger.can_hold(&smaller));
}
```

## 3. Choose the most informative assertion

### Boolean conditions

Use `assert!` when the expected result is naturally expressed as a Boolean
condition:

```rust
assert!(value.is_valid());
```

### Equality

Use `assert_eq!` when two values should be equal:

```rust
assert_eq!(actual, expected);
```

### Inequality

Use `assert_ne!` when two values should differ:

```rust
assert_ne!(actual, forbidden_value);
```

- **SHOULD** prefer `assert_eq!` or `assert_ne!` over a generic Boolean
  comparison because their failure output displays both values.
- Types used with `assert_eq!` and `assert_ne!` **MUST** implement `PartialEq`.
- To obtain readable failure diagnostics, those types **SHOULD** also implement
  `Debug`.
- For custom structs and enums, **SHOULD** derive these traits when appropriate:

```rust
#[derive(Debug, PartialEq)]
struct ResultValue {
    value: i32,
}
```

## 4. Add useful failure context

- **SHOULD** include a custom failure message when the default assertion output
  does not explain the relevant inputs or circumstances.
- Custom assertion messages **SHOULD** identify the input, expected behavior,
  and relevant state.

```rust
assert!(
    result.contains(name),
    "result did not contain the supplied name: {name}"
);
```

All arguments after the required assertion arguments are passed through Rust’s
formatting machinery.

```rust
assert_eq!(
    actual,
    expected,
    "unexpected result for input {input:?}"
);
```

## 5. Test expected panics precisely

Use `#[should_panic]` when panic behavior is part of the function’s contract:

```rust
#[test]
#[should_panic]
fn rejects_invalid_configuration() {
    create_configuration(0);
}
```

- **SHOULD** add `expected = "..."` to ensure the test passes for the intended
  panic rather than an unrelated panic.

```rust
#[test]
#[should_panic(expected = "value must be greater than zero")]
fn rejects_zero() {
    create_configuration(0);
}
```

- **MUST NOT** use a broad panic test when a more specific result-based
  assertion can verify the behavior.
- **SHOULD** prefer returning `Result` for recoverable errors and reserve panic
  tests for APIs intentionally specified to panic.

## 6. Return `Result` from tests when appropriate

A test may return `Result<(), E>`:

```rust
#[test]
fn parses_valid_input() -> Result<(), String> {
    let value = parse_value("42")?;

    if value == 42 {
        Ok(())
    } else {
        Err(format!("expected 42, got {value}"))
    }
}
```

- **SHOULD** return `Result` when the test performs fallible setup or when use
  of `?` improves clarity.
- A successful test **MUST** return `Ok(())`.
- A failed test **MUST** return `Err(...)`.
- **MUST NOT** combine a `Result`-returning test with `#[should_panic]`.
- To test that a `Result` is an error, **SHOULD** inspect or unwrap the error
  explicitly rather than relying on `#[should_panic]`.

## 7. Assume tests run concurrently

By default, Rust runs tests in parallel using multiple threads.

- **MUST** design tests so they do not depend on execution order.
- **MUST NOT** let tests compete over shared mutable state without
  synchronization.
- **MUST NOT** have multiple tests write to the same fixed file path,
  environment setting, port, database row, or other shared resource unless
  isolation is enforced.
- **SHOULD** generate unique resources for each test.
- **SHOULD** clean up external resources created by a test.

When concurrency cannot safely be supported, run tests with one test thread:

```bash
cargo test -- --test-threads=1
```

This is a fallback, not a substitute for proper test isolation.

## 8. Understand output capture

Rust normally captures output from successful tests.

```rust
println!("diagnostic information");
```

- Output from a failing test is displayed automatically.
- To display output from successful tests, run:

```bash
cargo test -- --show-output
```

- **SHOULD NOT** treat printed output as an assertion.
- **SHOULD** use assertions to determine pass or failure.
- Diagnostic output **SHOULD** be concise and relevant.

## 9. Use test filtering during development

Run tests whose names contain a given string:

```bash
cargo test parse
```

This may run one test or several matching tests.

- **SHOULD** give tests descriptive names so filtering is useful.
- Test names **SHOULD** describe the scenario and expected outcome.

Good examples:

```rust
fn empty_input_returns_error()
fn valid_rectangle_contains_smaller_rectangle()
fn value_above_limit_panics()
```

Less useful:

```rust
fn test_1()
fn works()
fn check_value()
```

## 10. Mark expensive tests as ignored

Tests that are slow, environment-dependent, or expensive may be annotated with
`#[ignore]`:

```rust
#[test]
#[ignore]
fn expensive_end_to_end_test() {
    // ...
}
```

Run only ignored tests with:

```bash
cargo test -- --ignored
```

Run all tests, including ignored tests, with:

```bash
cargo test -- --include-ignored
```

- **SHOULD** keep the default test suite fast enough to run frequently.
- **SHOULD** document why an ignored test is excluded.
- **MUST NOT** use `#[ignore]` merely to hide a failing test.
- A consistently failing test **MUST** be fixed, removed with justification, or
  tracked as an explicit known defect.

## 11. Separate unit tests from integration tests

### Unit tests

Unit tests exercise small pieces of code in isolation.

Place them in the same source file as the code, conventionally inside a test
module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_helper_works() {
        assert_eq!(internal_helper(2), 4);
    }
}
```

Rules:

- The module **SHOULD** be annotated with `#[cfg(test)]`.
- Test functions inside it **MUST** use `#[test]`.
- The test module **SHOULD** import parent-module items with `use super::*;`.
- Unit tests **MAY** test private functions because the nested test module can
  access items in its parent module.
- **SHOULD** test private functions directly only when doing so provides useful,
  stable verification.
- **SHOULD** prefer public-behavior tests when direct private-function tests
  would make refactoring unnecessarily difficult.

`#[cfg(test)]` ensures the test module is compiled only when tests are run.

### Integration tests

Integration tests exercise the crate through its public API.

Place them in the top-level `tests` directory:

```text
project/
├── Cargo.toml
├── src/
│   └── lib.rs
└── tests/
    └── public_api.rs
```

Example:

```rust
use my_crate::public_function;

#[test]
fn public_api_returns_expected_value() {
    assert_eq!(public_function(3), 6);
}
```

Rules:

- Each file in `tests/` is compiled as a separate crate.
- Integration tests **MUST** access the crate through its public API.
- Integration tests do not require `#[cfg(test)]`.
- **SHOULD** use integration tests to verify that public components work
  together correctly.
- **MUST NOT** depend on private implementation details.

## 12. Put reusable integration-test helpers in a module

A standalone file such as `tests/common.rs` is treated as an integration-test
crate. To avoid this, use a submodule:

```text
tests/
├── common/
│   └── mod.rs
└── public_api.rs
```

Then import it from an integration test:

```rust
mod common;

#[test]
fn public_behavior_works() {
    common::setup();
}
```

- Shared test setup code **SHOULD** live in a module such as
  `tests/common/mod.rs`.
- Helper modules **SHOULD NOT** contain independent tests unless they are
  intentionally meant to be separate test crates.
- Common helpers **SHOULD** remain small and should not conceal the important
  setup conditions of each test.

## 13. Structure binary projects for testability

A package containing only `src/main.rs` does not expose a library API that
integration tests can import.

For substantial binary applications:

- **SHOULD** place application logic in `src/lib.rs`.
- **SHOULD** keep `src/main.rs` small.
- `main` **SHOULD** call the public library API.
- Integration tests **SHOULD** test the library crate.

Typical structure:

```text
src/
├── lib.rs
└── main.rs
```

This separates testable logic from command-line startup and process-management
code.

## 14. Run the appropriate test scope

Run the complete test suite:

```bash
cargo test
```

Run a named integration-test target:

```bash
cargo test --test public_api
```

Run tests matching a name:

```bash
cargo test test_name_fragment
```

Pass options to the test executable after `--`:

```bash
cargo test -- --show-output
cargo test -- --test-threads=1
```

The distinction is:

- Arguments before `--` are interpreted by Cargo.
- Arguments after `--` are passed to the generated test binary.

## 15. Preserve test independence and determinism

Every test **MUST**:

- Produce the same result when run alone or with the full suite.
- Avoid dependence on test execution order.
- Avoid dependence on machine-specific state unless explicitly configured.
- Control nondeterministic inputs such as time, randomness, networking, and
  shared storage.
- Clearly establish all required preconditions.
- Fail for one understandable reason.

An AI agent **SHOULD** regard flaky tests as defects, not as acceptable
intermittent behavior.

## 16. Use failures as diagnostic information

When a test fails, the agent **MUST** examine:

- The test name.
- The panic or assertion message.
- The reported actual and expected values.
- Relevant source locations.
- Whether the failure is deterministic.
- Whether the implementation or the test expectation is incorrect.

The agent **MUST NOT** automatically change the assertion merely to make the
test pass. It must first determine whether the production code or the test
specification is wrong.

## 17. Apply this decision hierarchy

When adding tests, an AI agent **SHOULD** proceed in this order:

1. Identify the public behavior or contract.
2. Select representative normal, boundary, and invalid inputs.
3. Choose unit or integration scope.
4. Isolate state and external resources.
5. Execute the behavior.
6. Use the most informative assertion.
7. Add failure context where necessary.
8. Run the focused test.
9. Run the complete suite.
10. Investigate any failure rather than suppressing it.

## Compact compliance checklist

Before completing a Rust change, the agent **MUST** verify that:

- New behavior has appropriate tests.
- Tests have descriptive names.
- Assertions check meaningful outcomes.
- Panic tests specify the expected panic message where practical.
- Shared state does not make tests order-dependent.
- Unit tests are under `#[cfg(test)]`.
- Integration tests use only the public API.
- Reusable integration helpers are placed in a submodule.
- Ignored tests have a legitimate documented reason.
- `cargo test` passes without hiding or weakening failures.

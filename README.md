# spanr

A tool for procedural macro authors to be able to interactively view and debug
the `Span`s on generated code.

## Screenshot

![screenshot](screenshot.png)

## Example usage

```rust
#[proc_macro_attribute]
pub fn act_zero(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let res = match act_zero_impl(item) {
        Ok(tokens) => tokens,
        Err(e) => e.to_compile_error(),
    };
    // Save the visualization to a file
    spanr::save_html(res.clone(), "tokens.html").unwrap();
    res.into()
}
```

## Building

This crate relies on unstable features from the `proc-macro2` crate, so it must
be built using a nightly compiler, and the `RUSTFLAGS` environment variable
must be configured:

```bash
RUSTFLAGS='--cfg procmacro2_semver_exempt'
```

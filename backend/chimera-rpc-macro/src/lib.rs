//! The `#[rpc]` attribute, to be re-exported as `chimera_rpc::rpc`.
//! Production resource-table generation is migrated with the declaration roots.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod unified_command;

#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attr as unified_command::Options);
    let item = parse_macro_input!(item as syn::ItemFn);
    match unified_command::expand_with_options(item, options) {
        Ok(tokens) => TokenStream::from(tokens),
        Err(error) => TokenStream::from(error.to_compile_error()),
    }
}

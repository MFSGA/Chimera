use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, FnArg, ItemFn, Pat, ReturnType, Type, spanned::Spanned};

#[derive(Default)]
/// The arguments of `#[rpc(...)]`.
///
/// Only the implementation reads `result`, `owner`, `privacy` and `operation`. The other options
/// are authorization metadata, which the resource table generator reads from the declaration
/// together with its position in the namespace tree; the expansion uses the transports named by
/// `expose` only to require a transport-neutral signature.
pub struct Options {
    /// The effective exposure the generator derives; never parsed from the attribute.
    pub(crate) http: bool,
    pub(crate) local: bool,
    pub(crate) capabilities: Vec<syn::Ident>,
    pub(crate) optional_capabilities: Vec<syn::Ident>,
    result: bool,
    pub(crate) owner: bool,
    privacy: bool,
    operation: Option<syn::Path>,
    /// A query only reads. Every other command is a mutation.
    pub(crate) query: bool,
    /// Responses may disclose secrets; never part of admission.
    pub(crate) sensitive: bool,
    /// `access = read|write`: the level, when it differs from the one the effect derives.
    pub(crate) access: Option<syn::Ident>,
    /// `expose = [http, local]`: the transports beyond the desktop, a subset of the namespace's.
    pub(crate) expose: Option<Vec<syn::Ident>>,
}

impl Options {
    /// Whether HTTP is exposed: derived, or named within `expose`.
    pub(crate) fn names_http(&self) -> bool {
        self.http || self.exposes("http")
    }

    /// Whether the local socket is exposed: derived, or named within `expose`.
    pub(crate) fn names_local(&self) -> bool {
        self.local || self.exposes("local")
    }

    fn exposes(&self, target: &str) -> bool {
        self.expose
            .as_ref()
            .is_some_and(|targets| targets.iter().any(|value| value == target))
    }
}

/// Why the attribute rejects the options a declaration used to name itself.
pub(crate) const DERIVED_OPTION: &str = "the namespace tree derives the requirement and the exposure: remove `permission`, `http` and `local`, narrow the exposure with `expose = [...]` and change the level with `access = read|write`";

impl syn::parse::Parse for Options {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut options = Self::default();
        while !input.is_empty() {
            let option: syn::Ident = input.parse()?;
            match option.to_string().as_str() {
                "http" | "local" | "permission" => {
                    return Err(Error::new(option.span(), DERIVED_OPTION));
                }
                "capabilities" | "optional_capabilities" => {
                    let list;
                    syn::parenthesized!(list in input);
                    let values = list.parse_terminated(syn::Ident::parse, syn::Token![,])?;
                    if option == "capabilities" {
                        options.capabilities.extend(values);
                    } else {
                        options.optional_capabilities.extend(values);
                    }
                }
                "result" => options.result = true,
                "owner" => options.owner = true,
                "privacy" => options.privacy = true,
                "query" => options.query = true,
                "sensitive" => options.sensitive = true,
                "access" => {
                    input.parse::<syn::Token![=]>()?;
                    let access: syn::Ident = input.parse()?;
                    if !matches!(access.to_string().as_str(), "read" | "write") {
                        return Err(Error::new(access.span(), "`access` is `read` or `write`"));
                    }
                    if options.access.replace(access).is_some() {
                        return Err(Error::new(option.span(), "duplicate access"));
                    }
                }
                "expose" => {
                    input.parse::<syn::Token![=]>()?;
                    let list;
                    syn::bracketed!(list in input);
                    let targets: Vec<syn::Ident> = list
                        .parse_terminated(syn::Ident::parse, syn::Token![,])?
                        .into_iter()
                        .collect();
                    if let Some(target) = targets
                        .iter()
                        .find(|target| !matches!(target.to_string().as_str(), "http" | "local"))
                    {
                        return Err(Error::new(
                            target.span(),
                            "`expose` names `http` and `local`; the desktop is always exposed",
                        ));
                    }
                    if options.expose.replace(targets).is_some() {
                        return Err(Error::new(option.span(), "duplicate expose"));
                    }
                }
                "operation" => {
                    input.parse::<syn::Token![=]>()?;
                    if options.operation.is_some() {
                        return Err(Error::new(option.span(), "duplicate operation binding"));
                    }
                    options.operation = Some(input.parse()?);
                }
                _ => {
                    return Err(Error::new(
                        option.span(),
                        "expected result, owner, privacy, query, sensitive, access = read|write, expose = [...], operation = path, capabilities(...) or optional_capabilities(...)",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
pub(super) fn options(item: &ItemFn) -> syn::Result<Options> {
    item.attrs
        .iter()
        .find(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|s| s.ident == "rpc")
        })
        .filter(|attr| matches!(attr.meta, syn::Meta::List(_)))
        .map(|attr| attr.parse_args())
        .transpose()
        .map(Option::unwrap_or_default)
}

#[cfg(test)]
pub fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    let options = options(&item)?;
    expand_with_options(item, options)
}

pub(super) fn is_native(item: &ItemFn) -> bool {
    item.attrs.iter().any(|attr| {
        let path = &attr.path().segments;
        path.len() == 2 && path[0].ident == "tauri" && path[1].ident == "command"
    })
}

pub(super) fn validate(item: &ItemFn, options: &Options) -> syn::Result<()> {
    let original = &item.sig;
    if !options.privacy
        && success_output_type(&original.output, options.result).is_some_and(|ty| {
            is_context_type(ty, "RpcMutationOutcome")
                || is_context_type(ty, "RpcUnitMutationOutcome")
                || is_context_type(ty, "RpcProfileMutationOutcome")
                || is_context_type(ty, "RpcFolderMutationOutcome")
                || is_context_type(ty, "RpcAppUpdateOutcome")
                || is_context_type(ty, "RpcNetworkAutomationStatus")
                || is_context_type(ty, "MutationOutcome")
        })
    {
        return Err(Error::new(
            original.output.span(),
            "commands returning `RpcMutationOutcome`, `RpcAppUpdateOutcome` or `RpcNetworkAutomationStatus` must specify `privacy` (and raw `MutationOutcome` must wrap via `RpcMutationOutcome` + `privacy`)",
        ));
    }
    let native = is_native(item);
    let neutral_compatible = can_share_with_neutral(original, options.owner);
    if !native && (!neutral_compatible || original.inputs.iter().any(|argument| {
        matches!(argument, FnArg::Typed(argument) if (is_state(&argument.ty) && !is_context_type(&argument.ty, "RpcState")) || is_tauri_context(&argument.ty))
    })) {
        return Err(Error::new(original.span(), "a shared command requires a transport-neutral signature"));
    }
    if options.names_http() && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "exposing a command over HTTP requires a transport-neutral signature",
        ));
    }
    if options.names_local() && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "exposing a command over the local socket requires a transport-neutral signature",
        ));
    }
    Ok(())
}

pub fn expand_with_options(item: ItemFn, options: Options) -> syn::Result<TokenStream> {
    validate(&item, &options)?;
    let original = &item.sig;
    let name = &original.ident;
    let implementation_name = format_ident!("__unified_impl_{}", name);
    let neutral_handler = format_ident!("__unified_rpc_{}", name);
    let native = is_native(&item);
    let neutral_compatible = can_share_with_neutral(original, options.owner);
    let operation = options.operation.as_ref();
    let operation_trait = quote!(::chimera_rpc_protocol::operation::RpcOperation);
    let name_check = operation.map(|operation| {
        quote! {
            const _: () = {
                let actual = <#operation as #operation_trait>::NAME.as_bytes();
                let expected = stringify!(#name).as_bytes();
                assert!(actual.len() == expected.len(), "RPC operation name mismatch");
                let mut index = 0;
                while index < actual.len() {
                    assert!(actual[index] == expected[index], "RPC operation name mismatch");
                    index += 1;
                }
            };
        }
    });
    let tauri_handler = format_ident!("__unified_tauri_{}", name);
    let kind = if native {
        quote!(Native)
    } else {
        quote!(Shared)
    };

    let mut implementation = item.clone();
    implementation.attrs = item
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .cloned()
        .collect();
    implementation.vis = syn::Visibility::Inherited;
    implementation.sig.ident = implementation_name.clone();

    let mut wrapper_args = Vec::new();
    let mut neutral_args = Vec::new();
    let mut dependency_bindings = Vec::new();
    let mut fields = Vec::new();
    let mut field_names = Vec::new();
    let mut tauri_fields = Vec::new();
    let mut tauri_field_names = Vec::new();
    let mut neutral_supported =
        neutral_compatible && (options.names_http() || options.names_local() || !native);
    let mut tauri_supported = true;
    let mut uses_dependencies = false;

    for argument in implementation.sig.inputs.iter_mut() {
        let FnArg::Typed(argument) = argument else {
            return Err(Error::new(
                argument.span(),
                "self receivers are not supported",
            ));
        };
        let Pat::Ident(binding) = argument.pat.as_ref() else {
            return Err(Error::new(
                argument.pat.span(),
                "command parameters must be identifiers",
            ));
        };
        let ident = binding.ident.clone();

        if let Some(state_type) = state_inner_type(&argument.ty) {
            let state_name = type_name(&state_type);
            argument.ty = Box::new(syn::parse_quote!(&#state_type));
            wrapper_args.push(quote!(&*#ident));
            if state_name.as_deref() == Some("ChimeraClient") {
                uses_dependencies = true;
                neutral_args.push(quote!(&dependencies.client));
            } else if state_name.as_deref() == Some("Storage") {
                uses_dependencies = true;
                neutral_args.push(quote!(&dependencies.storage));
            } else if state_name.as_deref() == Some("PathResolver") {
                uses_dependencies = true;
                neutral_args.push(quote!(&dependencies.paths));
            } else if state_name.as_deref() == Some("HostEnvironment") {
                uses_dependencies = true;
                neutral_args.push(quote!(&dependencies.host_environment));
            } else if let Some(slot) = host_port_slot(state_name.as_deref()) {
                uses_dependencies = true;
                let slot = format_ident!("{slot}");
                dependency_bindings.push(quote! {
                    let #ident = dependencies.#slot.clone().ok_or_else(||
                        ::chimera_rpc_protocol::RpcError::unsupported(stringify!(#name)))?;
                });
                neutral_args.push(quote!(#ident.as_ref()));
            } else {
                neutral_supported = false;
            }
        } else if options.owner && is_context_type(&argument.ty, "RpcCaller") {
            *argument.ty = syn::parse_quote!(::chimera_rpc_protocol::RpcOwner);
            wrapper_args.push(quote!(#ident.into_owner()));
            dependency_bindings.push(quote!(let owner = caller.owner().clone();));
            neutral_args.push(quote!(owner.clone()));
        } else if is_context_type(&argument.ty, "RpcDescribeContext") {
            wrapper_args.push(quote!(#ident));
            neutral_args.push(quote!(meta.describe_context(caller.auth())));
        } else if is_context_type(&argument.ty, "AppHandle") {
            wrapper_args.push(quote!(#ident));
            neutral_supported = false;
        } else if options.owner && is_context_type(&argument.ty, "Window") {
            *argument.ty = syn::parse_quote!(::chimera_rpc_protocol::RpcOwner);
            wrapper_args.push(quote!(::chimera_rpc_protocol::RpcOwner::desktop(#ident.label())));
            dependency_bindings.push(quote!(let owner = caller.owner().clone();));
            neutral_args.push(quote!(owner.clone()));
        } else if is_context_type(&argument.ty, "Window")
            || is_context_type(&argument.ty, "Webview")
            || is_context_type(&argument.ty, "WebviewWindow")
        {
            wrapper_args.push(quote!(#ident));
            neutral_supported = false;
        } else if is_tauri_channel(&argument.ty) {
            // Decode the frontend descriptor, then bind it to the invoking webview.
            // Channel commands remain unsupported over HTTP.
            neutral_supported = false;
            wrapper_args.push(quote!(#ident));
            tauri_fields.push(quote!(#ident: ::tauri::ipc::JavaScriptChannelId));
            tauri_field_names.push(ident.clone());
        } else {
            let ty = argument.ty.clone();
            fields.push(quote!(#ident: #ty));
            field_names.push(ident.clone());
            tauri_fields.push(quote!(#ident: #ty));
            tauri_field_names.push(ident.clone());
            wrapper_args.push(quote!(#ident));
            neutral_args.push(quote!(#ident));
            if contains_reference(&argument.ty) {
                tauri_supported = false;
            }
        }
    }

    let generic_args = static_lifetime_args(original, &mut tauri_supported);
    let attrs = &item.attrs;
    let cfg_attrs = implementation.attrs.iter().collect::<Vec<_>>();
    let name_check = name_check.map(|check| {
        quote! {
            #(#cfg_attrs)*
            #check
        }
    });
    let vis = &item.vis;
    let block = &item.block;
    let wrapper_call = call(
        &implementation_name,
        &wrapper_args,
        original.asyncness.is_some(),
        None,
    );
    let wrapper = quote! {
        #(#attrs)*
        #vis #original {
            #wrapper_call
        }
    };
    implementation.block = block.clone();

    let params_parser = parser(&fields, &field_names, operation);
    let neutral_parse = params_parser;
    let neutral_call = call(
        &implementation_name,
        &neutral_args,
        original.asyncness.is_some(),
        None,
    );
    let neutral_enabled = neutral_supported && !matches!(original.output, ReturnType::Default);
    let neutral_handler_body = if neutral_enabled {
        let output = output_value(&neutral_call, &original.output, options.result);
        let response_type =
            operation.map(|operation| quote!(: <#operation as #operation_trait>::Response));
        let finish_output = finish_output_expr(options.privacy);
        quote! {
            let visibility = meta.visibility();
            #(#dependency_bindings)*
            #neutral_parse
            Ok(::std::boxed::Box::pin(async move {
                match #output {
                    Ok(output) => {
                        let output #response_type = output;
                        #finish_output
                    }
                    Err(error) => ::chimera_rpc::registry::RpcCompletion::unreported(Err(error)),
                }
            }))
        }
    } else {
        quote! { Err(::chimera_rpc_protocol::RpcError::unsupported(stringify!(#name))) }
    };

    let tauri_handler_definition = if !native {
        quote!()
    } else if tauri_supported {
        let mut state_bindings = Vec::new();
        let mut tauri_args = Vec::new();
        for argument in &original.inputs {
            let FnArg::Typed(argument) = argument else {
                continue;
            };
            let Pat::Ident(binding) = argument.pat.as_ref() else {
                continue;
            };
            let ident = &binding.ident;
            if let Some(inner) = state_inner_type(&argument.ty) {
                let state_name = format_ident!("__state_{}", ident);
                state_bindings
                    .push(quote!(let #state_name = ::tauri::Manager::state::<#inner>(&app);));
                tauri_args.push(quote!(&*#state_name));
            } else if is_context_type(&argument.ty, "AppHandle") {
                tauri_args.push(quote!(app.clone()));
            } else if options.owner && is_context_type(&argument.ty, "Window") {
                tauri_args.push(quote!(::chimera_rpc_protocol::RpcOwner::desktop(
                    window.label()
                )));
            } else if is_context_type(&argument.ty, "Window") {
                tauri_args.push(quote!(window.clone()));
            } else if is_context_type(&argument.ty, "Webview")
                || is_context_type(&argument.ty, "WebviewWindow")
            {
                tauri_args.push(quote!(webview.clone()));
            } else if is_tauri_channel(&argument.ty) {
                tauri_args.push(quote!(#ident.channel_on(webview.clone())));
            } else {
                tauri_args.push(quote!(#ident));
            }
        }
        let generic_args = if generic_args.is_empty() {
            None
        } else {
            Some(generic_args.as_slice())
        };
        let tauri_call = call(
            &implementation_name,
            &tauri_args,
            original.asyncness.is_some(),
            generic_args,
        );
        let output = output_value(&tauri_call, &original.output, options.result);
        let parser = parser(&tauri_fields, &tauri_field_names, operation);
        let response_type =
            operation.map(|operation| quote!(: <#operation as #operation_trait>::Response));
        let finish_output = finish_output_expr(options.privacy);
        quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                app: ::tauri::AppHandle,
                window: ::tauri::Window,
                webview: ::tauri::Webview,
                visibility: ::chimera_rpc_protocol::ResponseVisibility,
                params: ::serde_json::Value,
            ) -> ::std::result::Result<::chimera_rpc::registry::RpcFuture, ::chimera_rpc_protocol::RpcError> {
                #parser
                Ok(::std::boxed::Box::pin(async move {
                    #(#state_bindings)*
                    match #output {
                        Ok(output) => {
                            let output #response_type = output;
                            #finish_output
                        }
                        Err(error) => ::chimera_rpc::registry::RpcCompletion::unreported(Err(error)),
                    }
                }))
            }
        }
    } else {
        quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                _app: ::tauri::AppHandle,
                _window: ::tauri::Window,
                _webview: ::tauri::Webview,
                _visibility: ::chimera_rpc_protocol::ResponseVisibility,
                _params: ::serde_json::Value,
            ) -> ::std::result::Result<::chimera_rpc::registry::RpcFuture, ::chimera_rpc_protocol::RpcError> {
                Err(::chimera_rpc_protocol::RpcError::unsupported(stringify!(#name)))
            }
        }
    };

    let native_entry = native.then(|| {
        quote! {
            #(#cfg_attrs)*
            ::inventory::submit! {
                crate::unified_rpc::native::TauriCommandEntry {
                    name: stringify!(#name),
                    handler: #tauri_handler,
                }
            }
        }
    });

    let deps_param = if neutral_enabled && uses_dependencies {
        format_ident!("dependencies")
    } else {
        format_ident!("_dependencies")
    };
    let meta_param = if neutral_enabled {
        format_ident!("meta")
    } else {
        format_ident!("_meta")
    };
    let caller_param = if neutral_enabled {
        format_ident!("caller")
    } else {
        format_ident!("_caller")
    };
    let params_param = if neutral_enabled {
        format_ident!("params")
    } else {
        format_ident!("_params")
    };

    Ok(quote! {
        #implementation
        #wrapper

        #name_check

        #(#cfg_attrs)*
        fn #neutral_handler(
            #deps_param: ::std::sync::Arc<::chimera_rpc::registry::RpcDependencies>,
            #meta_param: ::chimera_rpc::registry::RpcCallMeta,
            #caller_param: ::chimera_rpc::registry::RpcCaller,
            #params_param: ::serde_json::Value,
        ) -> ::std::result::Result<::chimera_rpc::registry::RpcFuture, ::chimera_rpc_protocol::RpcError> {
            #neutral_handler_body
        }

        #tauri_handler_definition
        #native_entry

        #(#cfg_attrs)*
        ::inventory::submit! {
            ::chimera_rpc::registry::CommandEntry {
                name: stringify!(#name),
                kind: ::chimera_rpc_protocol::CommandKind::#kind,
                transport_neutral: #neutral_enabled,
                handler: #neutral_handler,
            }
        }
    })
}

fn call(
    name: &syn::Ident,
    args: &[TokenStream],
    is_async: bool,
    generic_args: Option<&[TokenStream]>,
) -> TokenStream {
    let generics = generic_args
        .map(|args| quote!(::<#(#args),*>))
        .unwrap_or_default();
    if is_async {
        quote!(#name #generics (#(#args),*).await)
    } else {
        quote!(#name #generics (#(#args),*))
    }
}

fn output_value(call: &TokenStream, output: &ReturnType, result: bool) -> TokenStream {
    match output {
        ReturnType::Type(_, ty) if result || is_result(ty) => {
            quote!(#call.map_err(|error| ::chimera_rpc::error::application_error_for(visibility, error)))
        }
        ReturnType::Type(_, _) => quote!(Ok(#call)),
        ReturnType::Default => quote!(Ok(#call)),
    }
}

fn success_output_type(output: &ReturnType, result: bool) -> Option<&Type> {
    match output {
        ReturnType::Type(_, ty) if result || is_result(ty) => {
            let Type::Path(path) = ty.as_ref() else {
                return None;
            };
            let segment = path
                .path
                .segments
                .last()
                .expect("a type path has a segment");
            match &segment.arguments {
                syn::PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| {
                    if let syn::GenericArgument::Type(ty) = arg {
                        Some(ty)
                    } else {
                        None
                    }
                }),
                _ => None,
            }
        }
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
        ReturnType::Default => None,
    }
}

fn finish_output_expr(privacy: bool) -> TokenStream {
    if privacy {
        quote! {
            let completion = ::chimera_rpc::privacy::RpcResponseProjection::summarize_completion(&output);
            ::chimera_rpc::registry::RpcCompletion {
                result: ::chimera_rpc::privacy::RpcResponseProjection::encode_for_visibility(
                    output,
                    visibility,
                ),
                completion,
            }
        }
    } else {
        quote! {
            ::chimera_rpc::registry::RpcCompletion::unreported(
                ::serde_json::to_value(&output).map_err(|error| {
                    ::chimera_rpc::error::application_error_for(visibility, error)
                }),
            )
        }
    }
}

fn parser(
    fields: &[TokenStream],
    names: &[syn::Ident],
    operation: Option<&syn::Path>,
) -> TokenStream {
    let operation_trait = quote!(::chimera_rpc_protocol::operation::RpcOperation);
    if fields.is_empty() {
        let unit_check = operation.map(|operation| {
            quote! {
                let (): <#operation as #operation_trait>::Request = ();
            }
        });
        quote! {
            if !params.is_null() && !matches!(&params, ::serde_json::Value::Object(map) if map.is_empty()) {
                return Err(::chimera_rpc_protocol::RpcError::invalid_params_for(
                    visibility,
                    "expected an empty input object",
                ));
            }
            #unit_check
        }
    } else {
        let args = if let Some(operation) = operation {
            quote!(type Args = <#operation as #operation_trait>::Request;)
        } else {
            quote! {
                #[derive(::serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args { #(#fields,)* }
            }
        };
        quote! {
            #args
            let Args { #(#names,)* } = ::serde_json::from_value(params)
                .map_err(|error| ::chimera_rpc_protocol::RpcError::invalid_params_for(visibility, error.to_string()))?;
        }
    }
}

fn can_share_with_neutral(signature: &syn::Signature, owner: bool) -> bool {
    if signature
        .generics
        .params
        .iter()
        .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)))
        || matches!(signature.output, ReturnType::Default)
    {
        return false;
    }
    for argument in &signature.inputs {
        let FnArg::Typed(argument) = argument else {
            return false;
        };
        if !matches!(argument.pat.as_ref(), Pat::Ident(_)) {
            return false;
        }
        if is_state(&argument.ty) {
            if ![
                "ChimeraClient",
                "Storage",
                "PathResolver",
                "HostEnvironment",
                "ConfigurationMutations",
                "HostIconFiles",
                "CoreDirectoryResolver",
                "ServiceInstallPrompt",
                "AccentColorQuery",
            ]
            .iter()
            .any(|name| is_state_of(&argument.ty, name))
            {
                return false;
            }
        } else if (owner && is_context_type(&argument.ty, "RpcCaller"))
            || is_context_type(&argument.ty, "RpcDescribeContext")
            || (owner && is_context_type(&argument.ty, "Window"))
        {
            continue;
        } else if contains_reference(&argument.ty) || is_tauri_context(&argument.ty) {
            return false;
        }
    }
    true
}

fn static_lifetime_args(signature: &syn::Signature, supported: &mut bool) -> Vec<TokenStream> {
    signature
        .generics
        .params
        .iter()
        .map(|param| match param {
            syn::GenericParam::Lifetime(_) => quote!('static),
            _ => {
                *supported = false;
                quote!()
            }
        })
        .collect()
}

fn is_tauri_context(ty: &Type) -> bool {
    ["AppHandle", "Window", "Webview", "WebviewWindow"]
        .iter()
        .any(|name| is_context_type(ty, name))
        || is_tauri_channel(ty)
}

fn is_tauri_channel(ty: &Type) -> bool {
    matches!(ty, Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == "Channel")
            && path.path.segments.iter().any(|segment| segment.ident == "tauri"))
}

fn is_context_type(ty: &Type, expected: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == expected))
}

fn state_inner_type(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "State" && segment.ident != "RpcState" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

fn is_state(ty: &Type) -> bool {
    state_inner_type(ty).is_some()
}

fn is_state_of(ty: &Type, expected: &str) -> bool {
    state_inner_type(ty).is_some_and(|inner| type_name(&inner).as_deref() == Some(expected))
}

fn host_port_slot(name: Option<&str>) -> Option<&'static str> {
    match name? {
        "HostIconFiles" => Some("icon_files"),
        "CoreDirectoryResolver" => Some("core_directory"),
        "ServiceInstallPrompt" => Some("service_prompt"),
        "AccentColorQuery" => Some("accent_color"),
        _ => None,
    }
}

fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::TraitObject(object) => object.bounds.iter().find_map(|bound| match bound {
            syn::TypeParamBound::Trait(bound) => bound
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        }),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn contains_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_))
}

fn is_result(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Result"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use quote::{ToTokens, format_ident};
    use syn::{Item, ItemFn};

    use super::expand;

    /// Permission, effect, targets and capabilities belong to the resource table; the registration
    /// says only what was implemented.
    #[test]
    fn registration_carries_only_implementation_facts() {
        for item in [
            syn::parse_quote! {
                #[chimera_rpc::rpc(query, sensitive, capabilities(clipboard), optional_capabilities(elevation), expose = [http, local])]
                fn shared(value: String) -> String { value }
            },
            syn::parse_quote! {
                #[chimera_rpc::rpc(capabilities(opener))]
                #[tauri::command]
                fn native(window: tauri::Window) -> bool { true }
            },
        ] {
            let item: ItemFn = item;
            let native = super::is_native(&item);
            let expanded = expand(item).unwrap().to_string();
            for metadata in [
                "Permission",
                "Resource ::",
                "CommandEffect ::",
                "RpcTarget ::",
                "ShellCapability ::",
                "ResourceMetadata",
            ] {
                assert!(!expanded.contains(metadata), "{metadata} in {expanded}");
            }
            let kind = if native { "Native" } else { "Shared" };
            assert!(expanded.contains(&format!(
                "kind : :: chimera_rpc_protocol :: CommandKind :: {kind}"
            )));
            assert!(expanded.contains(&format!("transport_neutral : {}", !native)));
        }
    }

    /// A command's transports may come from its namespace instead of the attribute: a shared
    /// command always builds its neutral handler, and a native one builds it when it names its
    /// `expose`.
    #[test]
    fn the_shared_handler_follows_the_exposure_the_attribute_names() {
        let neutral = |item: ItemFn| {
            let expanded = expand(item).unwrap().to_string();
            assert!(expanded.contains("transport_neutral : true"), "{expanded}");
            expanded.contains("__unified_impl_")
                && expanded.contains("fn __unified_rpc_")
                && !expanded.contains("RpcError :: unsupported (stringify ! (")
        };
        assert!(neutral(syn::parse_quote! {
            #[chimera_rpc::rpc(query)]
            fn shared(value: String) -> String { value }
        }));
        assert!(neutral(syn::parse_quote! {
            #[chimera_rpc::rpc(expose = [http, local])]
            #[tauri::command]
            fn native_exposed(value: String) -> String { value }
        }));
        let desktop_only = expand(syn::parse_quote! {
            #[chimera_rpc::rpc(expose = [])]
            #[tauri::command]
            fn native_desktop(value: String) -> String { value }
        })
        .unwrap()
        .to_string();
        assert!(
            desktop_only.contains("transport_neutral : false"),
            "{desktop_only}"
        );
    }

    #[test]
    fn a_declaration_without_a_permission_still_expands() {
        expand(syn::parse_quote! {
            #[chimera_rpc::rpc(expose = [http])]
            fn update() -> bool { true }
        })
        .expect("the resource table generator rejects it instead");
    }

    #[test]
    fn privacy_projection_requires_explicit_option_and_binds_via_trait() {
        for output in [
            quote::quote!(RpcMutationOutcome<()>),
            quote::quote!(Result<types::RpcMutationOutcome<String>>),
            quote::quote!(CommandResult<RpcAppUpdateOutcome>),
            quote::quote!(WrappedOutcomeAlias),
        ] {
            let item = syn::parse2(quote::quote! {
                #[chimera_rpc::rpc(privacy)]
                #[tauri::command]
                fn test() -> #output { todo!() }
            })
            .unwrap();
            let mut options = super::options(&item).unwrap();
            options.result = output.to_string().starts_with("CommandResult");
            let expanded = super::expand_with_options(item, options)
                .unwrap()
                .to_string();
            assert!(
                expanded.contains("RpcResponseProjection :: summarize_completion"),
                "{output}"
            );
            assert!(
                expanded.contains("RpcResponseProjection :: encode_for_visibility"),
                "{output}"
            );
        }

        for output in [
            quote::quote!(RpcMutationOutcome<()>),
            quote::quote!(Result<RpcMutationOutcome<String>>),
            quote::quote!(Result<RpcAppUpdateOutcome>),
            quote::quote!(Result<MutationOutcome<()>>),
        ] {
            let item = syn::parse2(quote::quote! {
                #[chimera_rpc::rpc]
                fn missing_privacy() -> #output { todo!() }
            })
            .unwrap();
            let options = super::options(&item).unwrap();
            assert!(
                super::expand_with_options(item, options).is_err(),
                "{output} without `privacy` must be rejected"
            );
        }

        for output in [quote::quote!(Result<()>), quote::quote!(CommitReceipt)] {
            let item = syn::parse2(quote::quote! {
                #[chimera_rpc::rpc]
                fn ordinary() -> #output { todo!() }
            })
            .unwrap();
            let expanded = expand(item).unwrap().to_string();
            assert!(!expanded.contains("RpcResponseProjection"), "{output}");
            assert!(expanded.contains("unreported"), "{output}");
        }
    }

    #[test]
    fn production_privacy_projection_commands_are_fixed() {
        let mut reported = crate::resource_table::production_functions()
            .into_iter()
            .filter(|item| {
                expand(item.clone())
                    .unwrap()
                    .to_string()
                    .contains("RpcResponseProjection :: summarize_completion")
            })
            .map(|item| item.sig.ident.to_string())
            .collect::<Vec<_>>();
        reported.sort();
        assert_eq!(
            reported,
            [
                "activate_profile",
                "clear_proxy_fixed",
                "create_profile",
                "create_profile_folder",
                "create_proxy_group_folder",
                "delete_profile",
                "import_profile",
                "patch_app_config",
                "patch_clash_config",
                "patch_clash_system_config",
                "patch_profile_metadata",
                "patch_remote_profile_options",
                "patch_runtime_access",
                "patch_runtime_overrides",
                "patch_system_settings",
                "reorder_profile",
                "reorder_profiles_by_list",
                "replace_profile_definition",
                "save_profile_file",
                "select_proxy",
                "set_global_transforms",
                "set_profile_layout",
                "set_profile_valid_fields",
                "set_proxy_group_layout",
                "set_proxy_mode",
                "update_profile",
            ]
        );
    }

    #[test]
    fn operation_bindings_use_the_existing_generator() {
        let expanded = expand(syn::parse_quote! {
            #[chimera_rpc::rpc(owner, result, operation = contracts::OpenLogSession, expose = [http, local])]
            fn open_log_session(caller: RpcCaller, client: RpcState<'_, ChimeraClient>, source: Source, request: OpenLogs) -> LogResult<LogSession> { todo!() }
        }).unwrap();
        syn::parse2::<syn::File>(expanded.clone()).unwrap();
        let expanded = expanded.to_string();
        assert!(expanded.contains("type Args = < contracts :: OpenLogSession as"));
        assert!(expanded.contains("let Args { source , request , }"));
        assert!(!expanded.contains("struct Args"));
        assert!(expanded.contains("owner . clone () , & dependencies . client , source , request"));
        assert!(expanded.contains(":: Response ="));
        assert!(expanded.contains(":: NAME . as_bytes ()"));
        assert_eq!(expanded.matches("registry :: CommandEntry {").count(), 1);
        assert_eq!(expanded.matches("inventory :: submit !").count(), 1);
        assert!(!expanded.contains("TauriCommandEntry"));
    }

    #[test]
    fn zero_argument_contracts_check_unit_without_changing_the_parser() {
        let expanded = expand(syn::parse_quote! {
            #[chimera_rpc::rpc(operation = contracts::GetProfiles, expose = [local])]
            fn get_profiles(client: RpcState<'_, ChimeraClient>) -> Result<Profiles> { todo!() }
        })
        .unwrap()
        .to_string();
        assert!(expanded.contains("params . is_null ()"));
        assert!(expanded.contains("map . is_empty ()"));
        assert!(expanded.contains("let () : < contracts :: GetProfiles as"));
        assert!(!expanded.contains("from_value (params)"));
    }

    #[test]
    fn explicit_result_alias_is_unwrapped_and_owner_is_injected() {
        let command = syn::parse_quote! {
            #[chimera_rpc::rpc(result, owner, expose = [http])]
            #[tauri::command]
            pub async fn query_logs(window: tauri::Window, request: Query) -> LogResult<Page> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();
        assert!(
            expanded.contains(
                "__unified_impl_query_logs (owner . clone () , request) . await . map_err"
            )
        );
        assert!(expanded.contains("RpcOwner :: desktop (window . label ())"));
        assert!(!expanded.contains("RpcError :: unsupported"));
        assert!(!expanded.contains("window : tauri :: Window , request : Query ,"));
    }

    #[test]
    fn dependency_free_commands_can_use_http() {
        let command = syn::parse_quote! { #[chimera_rpc::rpc(expose = [http])] pub fn functions() -> Vec<String> { vec![] } };
        assert!(
            !expand(command)
                .unwrap()
                .to_string()
                .contains("RpcError :: unsupported")
        );
    }

    #[test]
    fn the_path_resolver_state_is_served_from_the_http_dependencies() {
        let command = syn::parse_quote! {
            #[chimera_rpc::rpc(expose = [http])]
            #[tauri::command]
            pub fn is_tray_icon_set(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<bool> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("& dependencies . paths"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }

    #[test]
    fn channel_parameters_are_rebuilt_for_desktop_but_unsupported_over_http() {
        let command = syn::parse_quote! {
            #[chimera_rpc::rpc(owner)]
            #[tauri::command]
            pub fn stream(window: tauri::Window, on_chunk: tauri::ipc::Channel<String>) -> Result<()> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("on_chunk : :: tauri :: ipc :: JavaScriptChannelId"));
        assert!(expanded.contains("on_chunk . channel_on (webview . clone ())"));
        assert!(expanded.contains("RpcOwner :: desktop (window . label ())"));
        assert!(!expanded.contains("struct Args { window"));
        assert!(expanded.contains("RpcError :: unsupported (stringify ! (stream))"));
    }

    #[test]
    fn channel_parameter_json_parser_uses_camel_case_fields() {
        let command = syn::parse_quote! {
            #[chimera_rpc::rpc]
            #[tauri::command]
            pub fn stream(on_chunk: tauri::ipc::Channel<String>, page_size: usize) -> Result<()> {
                todo!()
            }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("rename_all = \"camelCase\""));
        assert!(expanded.contains("page_size : usize"));
        assert!(expanded.contains("from_value (params)"));
        assert!(expanded.contains("on_chunk : :: tauri :: ipc :: JavaScriptChannelId"));
    }

    #[test]
    fn http_requires_opt_in_and_accepts_only_lifetime_generics() {
        for (source, supported) in [
            (
                "#[tauri::command] #[chimera_rpc::rpc] pub fn open_that(path: String) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[chimera_rpc::rpc(expose = [http])] pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> { todo!() }",
                true,
            ),
            (
                "#[tauri::command] #[chimera_rpc::rpc(expose = [http])] pub fn generic<T>() -> Result<T> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[chimera_rpc::rpc(expose = [http])] pub fn generic<const N: usize>() -> Result<usize> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[chimera_rpc::rpc(expose = [http])] pub fn window(window: tauri::Window) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[chimera_rpc::rpc(expose = [http])] pub fn desktop(app: tauri::AppHandle) -> Result<()> { todo!() }",
                false,
            ),
        ] {
            let command: ItemFn = syn::parse_str(source).unwrap();
            if !supported && source.contains("expose = [http]") {
                assert_eq!(
                    expand(command).unwrap_err().to_string(),
                    "exposing a command over HTTP requires a transport-neutral signature",
                    "{source}"
                );
                continue;
            }
            let expanded = expand(command).unwrap().to_string();
            assert_eq!(
                !expanded.contains("RpcError :: unsupported"),
                supported,
                "{source}"
            );
        }
    }

    #[test]
    fn http_declaration_rejects_native_only_arguments() {
        for (arguments, options) in [
            (
                quote::quote!(on_chunk: tauri::ipc::Channel<String>),
                quote::quote!(expose = [http]),
            ),
            (
                quote::quote!(window: tauri::Window, on_chunk: tauri::ipc::Channel<String>),
                quote::quote!(owner, expose = [http]),
            ),
            (
                quote::quote!(app: tauri::AppHandle),
                quote::quote!(expose = [http]),
            ),
            (
                quote::quote!(window: tauri::Window),
                quote::quote!(expose = [http]),
            ),
            (
                quote::quote!(webview: tauri::Webview),
                quote::quote!(expose = [http]),
            ),
            (
                quote::quote!(window: tauri::WebviewWindow),
                quote::quote!(expose = [http]),
            ),
            (
                quote::quote!(state: tauri::State<'_, NativeState>),
                quote::quote!(expose = [http]),
            ),
            (quote::quote!(value: &str), quote::quote!(expose = [http])),
        ] {
            let command = syn::parse_quote! {
                #[chimera_rpc::rpc(#options)]
                #[tauri::command]
                fn native(#arguments) -> Result<()> { todo!() }
            };
            let error = expand(command).unwrap_err();
            assert_eq!(
                error.to_string(),
                "exposing a command over HTTP requires a transport-neutral signature",
                "{arguments}"
            );
        }
    }

    #[test]
    fn dispatchers_cover_each_command_category() {
        let mut commands = crate::resource_table::production_functions();
        for (name, targets) in [
            ("shared_desktop_fixture", quote::quote!()),
            ("shared_http_fixture", quote::quote!(expose = [http])),
            ("shared_local_fixture", quote::quote!(expose = [local])),
            ("shared_dual_fixture", quote::quote!(expose = [http, local])),
        ] {
            let name = format_ident!("{name}");
            commands.push(syn::parse_quote! {
                #[chimera_rpc::rpc(owner, #targets)]
                fn #name(client: RpcState<'_, ChimeraClient>, owner: RpcCaller, value: String) -> String { value }
            });
        }
        for command in commands {
            let native = super::is_native(&command);
            let needs_host_port = command.sig.inputs.iter().any(|arg| {
                let syn::FnArg::Typed(arg) = arg else {
                    return false;
                };
                super::state_inner_type(&arg.ty).is_some_and(|ty| {
                    super::host_port_slot(super::type_name(&ty).as_deref()).is_some()
                })
            });
            let name = command.sig.ident.to_string();
            let expanded =
                expand(command).unwrap_or_else(|error| panic!("failed to expand {name}: {error}"));
            let expanded = syn::parse2::<syn::File>(expanded)
                .unwrap_or_else(|error| panic!("failed to parse expansion for {name}: {error}"));
            let registration = expanded.to_token_stream().to_string();
            assert!(registration.contains(&format!("handler : __unified_rpc_{name}")));
            assert_eq!(
                registration.contains(&format!("handler : __unified_tauri_{name}")),
                native
            );
            if !native {
                assert!(!registration.contains(":: tauri"));
                assert!(registration.contains("transport_neutral : true"), "{name}");
                assert!(needs_host_port || !registration.contains("RpcError :: unsupported"));
            }
            assert!(
                !registration.contains("dispatch_audited"),
                "handlers must not audit twice"
            );
            assert!(
                !registration.contains("RpcAudit"),
                "the registry owns auditing"
            );
            let handler_name = if native {
                format_ident!("__unified_tauri_{name}")
            } else {
                format_ident!("__unified_rpc_{name}")
            };
            let handler = expanded
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Fn(function) if function.sig.ident == handler_name => Some(function),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing Tauri handler for {name}"));
            assert!(
                handler
                    .block
                    .to_token_stream()
                    .to_string()
                    .contains(&format!("__unified_impl_{name}")),
                "{name} must call its implementation even when a host port can be absent"
            );
            assert!(
                needs_host_port
                    || !handler
                        .block
                        .to_token_stream()
                        .to_string()
                        .contains("RpcError :: unsupported"),
                "Tauri command {name} was compiled into an unsupported dispatcher"
            );
        }
    }

    #[test]
    fn neutral_expansion_contains_no_tauri_types() {
        let command = syn::parse_quote! {
            #[chimera_rpc::rpc(owner, result, expose = [http, local])]
            #[specta::specta]
            pub async fn shared(
                client: RpcState<'_, ChimeraClient>, storage: RpcState<'_, Storage>, paths: RpcState<'_, PathResolver>,
                caller: RpcCaller, page_size: usize,
            ) -> SharedResult<String> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();
        for forbidden in [
            "tauri",
            "AppHandle",
            "JavaScriptChannelId",
            "TauriCommandEntry",
        ] {
            assert!(
                !expanded.contains(forbidden),
                "neutral expansion contains {forbidden}"
            );
        }
        assert!(expanded.contains("specta :: specta"));
        assert!(expanded.contains("& dependencies . client"));
        assert!(expanded.contains("& dependencies . storage"));
        assert!(expanded.contains("& dependencies . paths"));
        assert!(expanded.contains("owner . clone ()"));
        assert!(!expanded.contains("caller : RpcCaller , page_size : usize , }"));
        assert!(expanded.contains("rename_all = \"camelCase\""));
        assert!(expanded.contains("application_error_for (visibility"));
        assert!(!expanded.contains("deny_unknown_fields"));
        let empty = expand(syn::parse_quote! {
            #[chimera_rpc::rpc(expose = [http])] fn empty() -> bool { true }
        })
        .unwrap()
        .to_string();
        assert!(empty.contains("params . is_null ()"));
        assert!(empty.contains("map . is_empty ()"));
    }

    fn render_capability_inventory(inventories: &[(&str, BTreeMap<String, String>)]) -> String {
        // Linux supplies the baseline; every other platform is compared in full.
        let baseline = &inventories
            .iter()
            .find(|(platform, _)| *platform == "linux")
            .unwrap()
            .1;
        let mut snapshot = String::from(
            "# name | targets | required | optional | effect | permission\n# [baseline] uses Linux rows; platform differences: + row adds/replaces, - name removes.\n\n[baseline]\n",
        );
        for row in baseline.values() {
            snapshot.push_str(row);
            snapshot.push('\n');
        }
        for (platform, rows) in inventories {
            snapshot.push_str(&format!("\n[{platform}]\n"));
            for name in baseline.keys().filter(|name| !rows.contains_key(*name)) {
                snapshot.push_str(&format!("- {name}\n"));
            }
            for (name, row) in rows {
                if baseline.get(name) != Some(row) {
                    snapshot.push_str(&format!("+ {row}\n"));
                }
            }
        }
        snapshot
    }

    #[test]
    fn capability_inventory_records_platform_additions_changes_and_removals() {
        let rows = |values: &[&str]| -> BTreeMap<String, String> {
            values
                .iter()
                .map(|row| {
                    (
                        row.split(" | ").next().unwrap().to_string(),
                        row.to_string(),
                    )
                })
                .collect()
        };
        let snapshot = render_capability_inventory(&[
            (
                "windows",
                rows(&[
                    "added | desktop | - | - | mutation | system",
                    "changed | desktop | window | - | mutation | system",
                ]),
            ),
            (
                "linux",
                rows(&[
                    "changed | desktop | - | - | mutation | system",
                    "removed | desktop | - | - | mutation | system",
                ]),
            ),
            (
                "macos",
                rows(&[
                    "changed | desktop | - | - | mutation | system",
                    "removed | desktop | - | - | mutation | system",
                ]),
            ),
        ]);
        assert_eq!(
            snapshot,
            "# name | targets | required | optional | effect | permission\n# [baseline] uses Linux rows; platform differences: + row adds/replaces, - name removes.\n\n[baseline]\nchanged | desktop | - | - | mutation | system\nremoved | desktop | - | - | mutation | system\n\n[windows]\n- removed\n+ added | desktop | - | - | mutation | system\n+ changed | desktop | window | - | mutation | system\n\n[linux]\n\n[macos]\n"
        );
    }

    #[test]
    fn local_optional_elevation_expands_and_registers() {
        let expanded = expand(syn::parse_quote! {
            #[chimera_rpc::rpc(optional_capabilities(elevation), expose = [local])]
            fn profile_mutation(value: String) -> String { value }
        })
        .unwrap()
        .to_string();
        assert!(expanded.contains("transport_neutral : true"));
        assert!(expanded.contains("handler : __unified_rpc_profile_mutation"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }
}

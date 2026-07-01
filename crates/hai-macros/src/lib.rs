use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Attribute, Ident, ItemStruct, Meta, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

enum ToolArgs {
    None_,
    Type(syn::Type),
}

impl Parse for ToolArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        if ident != "args" {
            return Err(syn::Error::new(ident.span(), "expected `args`"));
        }
        let _: Token![=] = input.parse()?;

        if input.peek(Ident) {
            let lookahead = input.fork();
            let id: Ident = lookahead.parse()?;
            if id == "none" {
                input.parse::<Ident>()?;
                return Ok(Self::None_);
            }
        }

        let args_type: syn::Type = input.parse()?;
        Ok(Self::Type(args_type))
    }
}

fn first_doc_comment(attrs: &[Attribute]) -> String {
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let syn::Expr::Lit(expr) = &nv.value else {
            continue;
        };
        let syn::Lit::Str(s) = &expr.lit else {
            continue;
        };
        let text = s.value().trim().to_string();
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

fn camel_to_snake(name: &str) -> String {
    let mut result = String::new();
    for (i, c) in name.char_indices() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let struct_name = input.ident.clone();
    let name_lit = camel_to_snake(&struct_name.to_string());
    let description = first_doc_comment(&input.attrs);

    match syn::parse::<ToolArgs>(attr) {
        Ok(ToolArgs::None_) => {
            let expanded = quote! {
                #[derive(::std::fmt::Debug)]
                #input

                #[::async_trait::async_trait]
                impl AgentTool for #struct_name {
                    fn name(&self) -> &str { #name_lit }
                    fn description(&self) -> &str { #description }
                    fn schema(&self) -> ::std::option::Option<::serde_json::Value> { None }
                    async fn execute(
                        &self,
                        _args: ::serde_json::Value,
                    ) -> ::std::result::Result<::serde_json::Value, ToolError> {
                        self.exec().await
                    }
                }
            };
            TokenStream::from(expanded)
        }
        Ok(ToolArgs::Type(args_type)) => {
            expand_with_args(&struct_name, name_lit, description, input, args_type)
        }
        Err(_) => {
            let args_name_str = format!("{}Args", struct_name);
            let args_ident = Ident::new(&args_name_str, struct_name.span());
            let args_type = syn::parse2::<syn::Type>(quote!(#args_ident))
                .expect("failed to construct args type");
            expand_with_args(&struct_name, name_lit, description, input, args_type)
        }
    }
}

fn expand_with_args(
    struct_name: &Ident,
    name_lit: String,
    description: String,
    input: ItemStruct,
    args_type: syn::Type,
) -> TokenStream {
    let expanded = quote! {
        #[derive(::std::fmt::Debug)]
        #input

        #[::async_trait::async_trait]
        impl AgentTool for #struct_name {
            fn name(&self) -> &str { #name_lit }
            fn description(&self) -> &str { #description }
            fn schema(&self) -> ::std::option::Option<::serde_json::Value> {
                let s = ::serde_json::to_value(
                    ::schemars::schema_for!(#args_type)
                ).expect("valid schema");
                Some(s)
            }
            async fn execute(
                &self,
                args: ::serde_json::Value,
            ) -> ::std::result::Result<::serde_json::Value, ToolError> {
                let typed: #args_type = ::serde_json::from_value(args)?;
                self.exec(typed).await
            }
        }
    };
    TokenStream::from(expanded)
}

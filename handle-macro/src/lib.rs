use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input, spanned::Spanned};

#[proc_macro_derive(Handle)]
pub fn handle(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let enum_name = input.ident.clone();

    // derive handle name: FooEvent -> FooHandle
    let enum_name_str = enum_name.to_string();
    let handle_name_str = if enum_name_str.ends_with("Event") {
        enum_name_str.trim_end_matches("Event").to_string() + "Handle"
    } else {
        enum_name_str.clone() + "Handle"
    };
    let handle_ident = syn::Ident::new(&handle_name_str, enum_name.span());

    // ensure it's an enum
    let variants = if let Data::Enum(ref e) = input.data {
        &e.variants
    } else {
        return syn::Error::new_spanned(&input, "ActorHandle can only be derived for enums")
            .to_compile_error()
            .into();
    };

    // generate a method for each variant
    let methods = variants.iter().map(|v| {
        let variant_name = &v.ident;

        // convert CamelCase -> snake_case method name
        let method_name = syn::Ident::new(&to_snake_case(&variant_name.to_string()), variant_name.span());

        match &v.fields {
            Fields::Named(fields_named) => {
                let args = fields_named.named.iter().map(|f| {
                    let name = &f.ident;
                    let ty = &f.ty;
                    quote! { #name: #ty }
                });
                let arg_names = fields_named.named.iter().map(|f| &f.ident);

                quote! {
                    pub async fn #method_name(&self, #( #args ),* ) -> Result<()> {
                        self.tx.send(#enum_name::#variant_name { #( #arg_names ),* }).await?;
                        Ok(())
                    }
                }
            }
            Fields::Unnamed(fields_unnamed) => {
                let args = fields_unnamed.unnamed.iter().enumerate().map(|(i, f)| {
                    let name = syn::Ident::new(&format!("arg{}", i), f.span());
                    let ty = &f.ty;
                    quote! { #name: #ty }
                });
                let arg_names =
                    (0..fields_unnamed.unnamed.len()).map(|i| syn::Ident::new(&format!("arg{}", i), v.ident.span()));

                quote! {
                    pub async fn #method_name(&self, #( #args ),* ) -> Result<()> {
                        self.tx.send(#enum_name::#variant_name( #( #arg_names ),* )).await?;
                        Ok(())
                    }
                }
            }
            Fields::Unit => {
                quote! {
                    pub async fn #method_name(&self) -> Result<()> {
                        self.tx.send(#enum_name::#variant_name).await?;
                        Ok(())
                    }
                }
            }
        }
    });

    let expanded = quote! {
        #[derive(Debug, Clone)]
        pub struct #handle_ident {
            tx: tokio::sync::mpsc::Sender<#enum_name>,
        }

        impl #handle_ident {
            #( #methods )*
        }
    };

    TokenStream::from(expanded)
}

// simple CamelCase -> snake_case
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

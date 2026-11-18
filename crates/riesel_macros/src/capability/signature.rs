use syn::spanned::Spanned;
use syn::{GenericArgument, PathArguments, ReturnType, Type};

pub(super) fn result_parts(output: &ReturnType) -> syn::Result<(&Type, &Type)> {
    let ReturnType::Type(_, result) = output else {
        return Err(syn::Error::new(
            output.span(),
            "capability methods must return Result<T, E>",
        ));
    };
    let Type::Path(path) = &**result else {
        return Err(syn::Error::new(
            result.span(),
            "capability methods must return Result<T, E>",
        ));
    };
    let Some(last) = path.path.segments.last() else {
        return Err(syn::Error::new(path.span(), "expected Result<T, E>"));
    };
    if last.ident != "Result" {
        return Err(syn::Error::new(
            last.span(),
            "capability methods must return Result<T, E>",
        ));
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return Err(syn::Error::new(
            last.span(),
            "Result requires output and error types",
        ));
    };
    let types: Vec<_> = args
        .args
        .iter()
        .filter_map(|item| match item {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect();
    if types.len() != 2 || args.args.len() != 2 {
        return Err(syn::Error::new(
            args.span(),
            "Result requires two concrete types",
        ));
    }
    Ok((types[0], types[1]))
}

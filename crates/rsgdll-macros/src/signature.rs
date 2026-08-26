use syn::{GenericArgument, PathArguments, Type};

pub(crate) fn is_main_thread_context(ty: &Type) -> bool {
    is_mut_reference_to(ty, "MainThread")
}

pub(crate) fn is_stack_frame_context(ty: &Type) -> bool {
    is_mut_reference_to(ty, "StackFrame")
}

fn is_mut_reference_to(ty: &Type, expected: &str) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    if reference.mutability.is_none() {
        return false;
    }
    let Type::Path(path) = reference.elem.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == expected)
}

pub(crate) fn syntactic_result_ok_type(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let ok = types.next()?;
    types.next()?;
    types.next().is_none().then_some(ok)
}

pub(crate) fn is_unit(ty: Option<&Type>) -> bool {
    match ty {
        None => true,
        Some(Type::Tuple(tuple)) => tuple.elems.is_empty(),
        Some(_) => false,
    }
}

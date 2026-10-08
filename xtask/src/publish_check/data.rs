use std::collections::BTreeMap;
use syn::{parse::Parser, visit::Visit};

pub(super) type Literals = BTreeMap<usize, Vec<(usize, usize)>>;

pub(super) fn named_literals(text: &str, patterns: &[&str]) -> Literals {
    let Ok(file) = syn::parse_file(text) else {
        return Literals::new();
    };
    let mut named = NamedData {
        patterns,
        literals: Literals::new(),
    };
    named.visit_file(&file);
    named.literals
}

struct NamedData<'a> {
    patterns: &'a [&'a str],
    literals: Literals,
}

impl NamedData<'_> {
    fn is_pattern(&self, name: &str) -> bool {
        let upper = name.to_ascii_uppercase();
        self.patterns.iter().any(|word| upper.contains(word))
    }

    fn expression(&mut self, expression: &syn::Expr) {
        struct Integers<'a>(&'a mut Literals);
        impl<'ast> Visit<'ast> for Integers<'_> {
            fn visit_lit_int(&mut self, literal: &'ast syn::LitInt) {
                let span = literal.span();
                let start = span.start();
                let end = span.end();
                if start.line == end.line {
                    self.0
                        .entry(start.line)
                        .or_default()
                        .push((start.column, end.column));
                }
            }
        }
        Integers(&mut self.literals).visit_expr(expression);
    }
}

impl<'ast> Visit<'ast> for NamedData<'_> {
    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if expression.mac.path.is_ident("vec")
            && let Ok(elements) =
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                    .parse2(expression.mac.tokens.clone())
        {
            for element in &elements {
                self.visit_expr(element);
            }
        }
        syn::visit::visit_expr_macro(self, expression);
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if self.is_pattern(&item.ident.to_string()) {
            self.expression(&item.expr);
        }
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if self.is_pattern(&item.ident.to_string()) {
            self.expression(&item.expr);
        }
        syn::visit::visit_item_static(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let syn::Pat::Ident(binding) = &local.pat
            && self.is_pattern(&binding.ident.to_string())
            && let Some(initializer) = &local.init
        {
            self.expression(&initializer.expr);
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = call.func.as_ref()
            && function.path.segments.last().is_some_and(|segment| {
                matches!(segment.ident.to_string().as_str(), "rgb24" | "argb32")
            })
        {
            for argument in &call.args {
                self.expression(argument);
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

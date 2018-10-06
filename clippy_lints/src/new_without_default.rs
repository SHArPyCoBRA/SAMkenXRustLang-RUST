// Copyright 2014-2018 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


use crate::rustc::hir::def_id::DefId;
use crate::rustc::hir;
use crate::rustc::lint::{LateContext, LateLintPass, LintArray, LintPass, in_external_macro, LintContext};
use crate::rustc::{declare_tool_lint, lint_array};
use if_chain::if_chain;
use crate::rustc::ty::{self, Ty};
use crate::syntax::source_map::Span;
use crate::utils::paths;
use crate::utils::{get_trait_def_id, implements_trait, return_ty, same_tys, span_lint_and_then};
use crate::utils::sugg::DiagnosticBuilderExt;
use crate::rustc_errors::Applicability;

/// **What it does:** Checks for types with a `fn new() -> Self` method and no
/// implementation of
/// [`Default`](https://doc.rust-lang.org/std/default/trait.Default.html).
///
/// **Why is this bad?** The user might expect to be able to use
/// [`Default`](https://doc.rust-lang.org/std/default/trait.Default.html) as the
/// type can be constructed without arguments.
///
/// **Known problems:** Hopefully none.
///
/// **Example:**
///
/// ```rust
/// struct Foo(Bar);
///
/// impl Foo {
///     fn new() -> Self {
///         Foo(Bar::new())
///     }
/// }
/// ```
///
/// Instead, use:
///
/// ```rust
/// struct Foo(Bar);
///
/// impl Default for Foo {
///     fn default() -> Self {
///         Foo(Bar::new())
///     }
/// }
/// ```
///
/// You can also have `new()` call `Default::default()`.
declare_clippy_lint! {
    pub NEW_WITHOUT_DEFAULT,
    style,
    "`fn new() -> Self` method without `Default` implementation"
}

/// **What it does:** Checks for types with a `fn new() -> Self` method
/// and no implementation of
/// [`Default`](https://doc.rust-lang.org/std/default/trait.Default.html),
/// where the `Default` can be derived by `#[derive(Default)]`.
///
/// **Why is this bad?** The user might expect to be able to use
/// [`Default`](https://doc.rust-lang.org/std/default/trait.Default.html) as the
/// type can be constructed without arguments.
///
/// **Known problems:** Hopefully none.
///
/// **Example:**
///
/// ```rust
/// struct Foo;
///
/// impl Foo {
///     fn new() -> Self {
///         Foo
///     }
/// }
/// ```
///
/// Just prepend `#[derive(Default)]` before the `struct` definition.
declare_clippy_lint! {
    pub NEW_WITHOUT_DEFAULT_DERIVE,
    style,
    "`fn new() -> Self` without `#[derive]`able `Default` implementation"
}

#[derive(Copy, Clone)]
pub struct NewWithoutDefault;

impl LintPass for NewWithoutDefault {
    fn get_lints(&self) -> LintArray {
        lint_array!(NEW_WITHOUT_DEFAULT, NEW_WITHOUT_DEFAULT_DERIVE)
    }
}

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for NewWithoutDefault {
    fn check_item(&mut self, cx: &LateContext<'a, 'tcx>, item: &'tcx hir::Item) {
        if let hir::ItemKind::Impl(_, _, _, _, None, _, ref items) = item.node {
            for assoc_item in items {
                if let hir::AssociatedItemKind::Method { has_self: false } = assoc_item.kind {
                    let impl_item = cx.tcx.hir.impl_item(assoc_item.id);
                    if in_external_macro(cx.sess(), impl_item.span) {
                        return;
                    }
                    if let hir::ImplItemKind::Method(ref sig, _) = impl_item.node {
                        let name = impl_item.ident.name;
                        let id = impl_item.id;
                        if sig.header.constness == hir::Constness::Const {
                            // can't be implemented by default
                            return;
                        }
                        if impl_item.generics.params.iter().any(|gen| match gen.kind {
                            hir::GenericParamKind::Type { .. } => true,
                            _ => false
                        }) {
                            // when the result of `new()` depends on a type parameter we should not require
                            // an
                            // impl of `Default`
                            return;
                        }
                        if sig.decl.inputs.is_empty() && name == "new" && cx.access_levels.is_reachable(id) {
                            let self_ty = cx.tcx
                                .type_of(cx.tcx.hir.local_def_id(cx.tcx.hir.get_parent(id)));
                            if_chain! {
                                if same_tys(cx, self_ty, return_ty(cx, id));
                                if let Some(default_trait_id) = get_trait_def_id(cx, &paths::DEFAULT_TRAIT);
                                if !implements_trait(cx, self_ty, default_trait_id, &[]);
                                then {
                                    if let Some(sp) = can_derive_default(self_ty, cx, default_trait_id) {
                                        span_lint_and_then(
                                            cx,
                                            NEW_WITHOUT_DEFAULT_DERIVE,
                                            impl_item.span,
                                            &format!("you should consider deriving a `Default` implementation for `{}`", self_ty),
                                            |db| {
                                                db.suggest_item_with_attr(
                                                    cx,
                                                    sp,
                                                    "try this",
                                                    "#[derive(Default)]",
                                                    Applicability::MaybeIncorrect,
                                                );
                                            });
                                    } else {
                                        span_lint_and_then(
                                            cx,
                                            NEW_WITHOUT_DEFAULT,
                                            impl_item.span,
                                            &format!("you should consider adding a `Default` implementation for `{}`", self_ty),
                                            |db| {
                                                db.suggest_prepend_item(
                                                    cx,
                                                    item.span,
                                                    "try this",
                                                    &create_new_without_default_suggest_msg(self_ty),
                                                    Applicability::MaybeIncorrect,
                                                );
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn create_new_without_default_suggest_msg(ty: Ty<'_>) -> String {
    #[rustfmt::skip]
    format!(
"impl Default for {} {{
    fn default() -> Self {{
        Self::new()
    }}
}}", ty)
}

fn can_derive_default<'t, 'c>(ty: Ty<'t>, cx: &LateContext<'c, 't>, default_trait_id: DefId) -> Option<Span> {
    match ty.sty {
        ty::Adt(adt_def, substs) if adt_def.is_struct() => {
            for field in adt_def.all_fields() {
                let f_ty = field.ty(cx.tcx, substs);
                if !implements_trait(cx, f_ty, default_trait_id, &[]) {
                    return None;
                }
            }
            Some(cx.tcx.def_span(adt_def.did))
        },
        _ => None,
    }
}

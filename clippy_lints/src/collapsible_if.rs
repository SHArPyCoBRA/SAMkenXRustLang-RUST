// Copyright 2014-2018 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


//! Checks for if expressions that contain only an if expression.
//!
//! For example, the lint would catch:
//!
//! ```rust,ignore
//! if x {
//!     if y {
//!         println!("Hello world");
//!     }
//! }
//! ```
//!
//! This lint is **warn** by default

use crate::rustc::lint::{EarlyContext, EarlyLintPass, LintArray, LintPass};
use crate::rustc::{declare_tool_lint, lint_array};
use if_chain::if_chain;
use crate::syntax::ast;

use crate::utils::{in_macro, snippet_block, span_lint_and_sugg, span_lint_and_then};
use crate::utils::sugg::Sugg;
use crate::rustc_errors::Applicability;

/// **What it does:** Checks for nested `if` statements which can be collapsed
/// by `&&`-combining their conditions and for `else { if ... }` expressions
/// that
/// can be collapsed to `else if ...`.
///
/// **Why is this bad?** Each `if`-statement adds one level of nesting, which
/// makes code look more complex than it really is.
///
/// **Known problems:** None.
///
/// **Example:**
/// ```rust,ignore
/// if x {
///     if y {
///         …
///     }
/// }
///
/// // or
///
/// if x {
///     …
/// } else {
///     if y {
///         …
///     }
/// }
/// ```
///
/// Should be written:
///
/// ```rust.ignore
/// if x && y {
///     …
/// }
///
/// // or
///
/// if x {
///     …
/// } else if y {
///     …
/// }
/// ```
declare_clippy_lint! {
    pub COLLAPSIBLE_IF,
    style,
    "`if`s that can be collapsed (e.g. `if x { if y { ... } }` and `else { if x { ... } }`)"
}

#[derive(Copy, Clone)]
pub struct CollapsibleIf;

impl LintPass for CollapsibleIf {
    fn get_lints(&self) -> LintArray {
        lint_array!(COLLAPSIBLE_IF)
    }
}

impl EarlyLintPass for CollapsibleIf {
    fn check_expr(&mut self, cx: &EarlyContext<'_>, expr: &ast::Expr) {
        if !in_macro(expr.span) {
            check_if(cx, expr)
        }
    }
}

fn check_if(cx: &EarlyContext<'_>, expr: &ast::Expr) {
    match expr.node {
        ast::ExprKind::If(ref check, ref then, ref else_) => if let Some(ref else_) = *else_ {
            check_collapsible_maybe_if_let(cx, else_);
        } else {
            check_collapsible_no_if_let(cx, expr, check, then);
        },
        ast::ExprKind::IfLet(_, _, _, Some(ref else_)) => {
            check_collapsible_maybe_if_let(cx, else_);
        },
        _ => (),
    }
}

fn check_collapsible_maybe_if_let(cx: &EarlyContext<'_>, else_: &ast::Expr) {
    if_chain! {
        if let ast::ExprKind::Block(ref block, _) = else_.node;
        if let Some(else_) = expr_block(block);
        if !in_macro(else_.span);
        then {
            match else_.node {
                ast::ExprKind::If(..) | ast::ExprKind::IfLet(..) => {
                    span_lint_and_sugg(cx,
                                       COLLAPSIBLE_IF,
                                       block.span,
                                       "this `else { if .. }` block can be collapsed",
                                       "try",
                                       snippet_block(cx, else_.span, "..").into_owned());
                }
                _ => (),
            }
        }
    }
}

fn check_collapsible_no_if_let(cx: &EarlyContext<'_>, expr: &ast::Expr, check: &ast::Expr, then: &ast::Block) {
    if_chain! {
        if let Some(inner) = expr_block(then);
        if let ast::ExprKind::If(ref check_inner, ref content, None) = inner.node;
        then {
            if expr.span.ctxt() != inner.span.ctxt() {
                return;
            }
            span_lint_and_then(cx, COLLAPSIBLE_IF, expr.span, "this if statement can be collapsed", |db| {
                let lhs = Sugg::ast(cx, check, "..");
                let rhs = Sugg::ast(cx, check_inner, "..");
                db.span_suggestion_with_applicability(
                    expr.span,
                    "try",
                    format!(
                        "if {} {}",
                        lhs.and(&rhs),
                        snippet_block(cx, content.span, ".."),
                    ),
                    Applicability::MachineApplicable, // snippet
                );
            });
        }
    }
}

/// If the block contains only one expression, return it.
fn expr_block(block: &ast::Block) -> Option<&ast::Expr> {
    let mut it = block.stmts.iter();

    if let (Some(stmt), None) = (it.next(), it.next()) {
        match stmt.node {
            ast::StmtKind::Expr(ref expr) | ast::StmtKind::Semi(ref expr) => Some(expr),
            _ => None,
        }
    } else {
        None
    }
}

use rustc_hash::FxHashSet;

use ruff_diagnostics::{Diagnostic, Violation};
use ruff_macros::{derive_message_formats, violation};
use ruff_python_ast::comparable::ComparableExpr;
use ruff_python_ast::helpers::contains_effect;
use ruff_python_ast::{self as ast, CmpOp, ElifElseClause, Expr, Stmt};
use ruff_python_semantic::analyze::typing::{is_sys_version_block, is_type_checking_block};
use ruff_text_size::Ranged;

use crate::checkers::ast::Checker;

/// ## What it does
/// Checks for three or more consecutive if-statements with direct returns
///
/// ## Why is this bad?
/// These can be simplified by using a dictionary
///
/// ## Example
/// ```python
/// if x == 1:
///     return "Hello"
/// elif x == 2:
///     return "Goodbye"
/// else:
///     return "Goodnight"
/// ```
///
/// Use instead:
/// ```python
/// return {1: "Hello", 2: "Goodbye"}.get(x, "Goodnight")
/// ```
#[violation]
pub struct IfElseBlockInsteadOfDictLookup;

impl Violation for IfElseBlockInsteadOfDictLookup {
    #[derive_message_formats]
    fn message(&self) -> String {
        format!("Use a dictionary instead of consecutive `if` statements")
    }
}
/// SIM116
pub(crate) fn if_else_block_instead_of_dict_lookup(checker: &mut Checker, stmt_if: &ast::StmtIf) {
    // Throughout this rule:
    // * Each if or elif statement's test must consist of a constant equality check with the same variable.
    // * Each if or elif statement's body must consist of a single `return`.
    // * The else clause must be empty, or a single `return`.
    let ast::StmtIf {
        body,
        test,
        elif_else_clauses,
        ..
    } = stmt_if;

    let Expr::Compare(ast::ExprCompare {
        left,
        ops,
        comparators,
        range: _,
    }) = test.as_ref()
    else {
        return;
    };
    let Expr::Name(ast::ExprName { id: target, .. }) = left.as_ref() else {
        return;
    };
    if ops != &[CmpOp::Eq] {
        return;
    }
    let [expr] = comparators.as_slice() else {
        return;
    };
    if !expr.is_literal_expr() {
        return;
    }
    let [Stmt::Return(ast::StmtReturn { value, range: _ })] = body.as_slice() else {
        return;
    };

    if value
        .as_ref()
        .is_some_and(|value| contains_effect(value, |id| checker.semantic().is_builtin(id)))
    {
        return;
    }

    // Avoid suggesting ternary for `if sys.version_info >= ...`-style checks.
    if is_sys_version_block(stmt_if, checker.semantic()) {
        return;
    }

    // Avoid suggesting ternary for `if TYPE_CHECKING:`-style checks.
    if is_type_checking_block(stmt_if, checker.semantic()) {
        return;
    }

    // The `expr` was checked to be a literal above, so this is safe.
    let mut literals: FxHashSet<ComparableExpr> = FxHashSet::default();
    literals.insert(expr.into());

    for clause in elif_else_clauses {
        let ElifElseClause { test, body, .. } = clause;
        let [Stmt::Return(ast::StmtReturn { value, range: _ })] = body.as_slice() else {
            return;
        };

        match test.as_ref() {
            // `else`
            None => {
                // The else must also be a single effect-free return statement
                let [Stmt::Return(ast::StmtReturn { value, range: _ })] = body.as_slice() else {
                    return;
                };
                if value.as_ref().is_some_and(|value| {
                    contains_effect(value, |id| checker.semantic().is_builtin(id))
                }) {
                    return;
                };
            }
            // `elif`
            Some(Expr::Compare(ast::ExprCompare {
                left,
                ops,
                comparators,
                range: _,
            })) => {
                let Expr::Name(ast::ExprName { id, .. }) = left.as_ref() else {
                    return;
                };
                if id != target || ops != &[CmpOp::Eq] {
                    return;
                }
                let [expr] = comparators.as_slice() else {
                    return;
                };
                if !expr.is_literal_expr() {
                    return;
                }

                if value.as_ref().is_some_and(|value| {
                    contains_effect(value, |id| checker.semantic().is_builtin(id))
                }) {
                    return;
                };

                // The `expr` was checked to be a literal above, so this is safe.
                literals.insert(expr.into());
            }
            // Different `elif`
            _ => {
                return;
            }
        }
    }

    if literals.len() < 3 {
        return;
    }

    checker.diagnostics.push(Diagnostic::new(
        IfElseBlockInsteadOfDictLookup,
        stmt_if.range(),
    ));
}

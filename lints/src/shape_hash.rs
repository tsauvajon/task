use std::hash::{Hash, Hasher};

use rustc_hir::{Block, Expr, ExprKind, Stmt, StmtKind};

pub fn shape_hash_stmt(stmt: &Stmt<'_>) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    hash_stmt(&mut hasher, stmt);
    hasher.finish()
}

pub fn shape_hash_expr_into(hasher: &mut impl Hasher, expr: &Expr<'_>) {
    hash_expr(hasher, expr);
}

fn hash_stmt(hasher: &mut impl Hasher, stmt: &Stmt<'_>) {
    std::mem::discriminant(&stmt.kind).hash(hasher);
    match &stmt.kind {
        StmtKind::Let(local) => {
            local.ty.is_some().hash(hasher);
            if let Some(init) = local.init {
                hash_expr(hasher, init);
            }
            if let Some(els) = local.els {
                hash_block(hasher, els);
            }
        }
        StmtKind::Item(_) => {}
        StmtKind::Expr(expr) | StmtKind::Semi(expr) => hash_expr(hasher, expr),
    }
}

fn hash_block(hasher: &mut impl Hasher, block: &Block<'_>) {
    block.stmts.len().hash(hasher);
    for stmt in block.stmts {
        hash_stmt(hasher, stmt);
    }
    if let Some(tail) = block.expr {
        hash_expr(hasher, tail);
    }
}

#[expect(clippy::cognitive_complexity, reason = "HIR expression dispatcher")]
fn hash_expr(hasher: &mut impl Hasher, expr: &Expr<'_>) {
    std::mem::discriminant(&expr.kind).hash(hasher);
    match &expr.kind {
        ExprKind::Lit(lit) => std::mem::discriminant(&lit.node).hash(hasher),
        ExprKind::Call(func, args) => {
            hash_expr(hasher, func);
            args.len().hash(hasher);
            for arg in *args {
                hash_expr(hasher, arg);
            }
        }
        ExprKind::MethodCall(path, receiver, args, _) => {
            path.ident.name.hash(hasher);
            hash_expr(hasher, receiver);
            args.len().hash(hasher);
            for arg in *args {
                hash_expr(hasher, arg);
            }
        }
        ExprKind::Binary(op, left, right) => {
            std::mem::discriminant(&op.node).hash(hasher);
            hash_expr(hasher, left);
            hash_expr(hasher, right);
        }
        ExprKind::AssignOp(op, left, right) => {
            std::mem::discriminant(&op.node).hash(hasher);
            hash_expr(hasher, left);
            hash_expr(hasher, right);
        }
        ExprKind::Unary(op, inner) => {
            std::mem::discriminant(op).hash(hasher);
            hash_expr(hasher, inner);
        }
        ExprKind::Path(qpath) => std::mem::discriminant(qpath).hash(hasher),
        ExprKind::If(cond, then, els) => {
            hash_expr(hasher, cond);
            hash_expr(hasher, then);
            els.is_some().hash(hasher);
            if let Some(else_expr) = els {
                hash_expr(hasher, else_expr);
            }
        }
        ExprKind::Match(scrutinee, arms, _) => {
            hash_expr(hasher, scrutinee);
            arms.len().hash(hasher);
            for arm in *arms {
                if let Some(guard) = arm.guard {
                    hash_expr(hasher, guard);
                }
                hash_expr(hasher, arm.body);
            }
        }
        ExprKind::Block(block, _) | ExprKind::Loop(block, _, _, _) => hash_block(hasher, block),
        ExprKind::AddrOf(_, mutability, inner) => {
            std::mem::discriminant(mutability).hash(hasher);
            hash_expr(hasher, inner);
        }
        ExprKind::Field(inner, field) => {
            hash_expr(hasher, inner);
            field.name.hash(hasher);
        }
        ExprKind::Index(left, right, _) | ExprKind::Assign(left, right, _) => {
            hash_expr(hasher, left);
            hash_expr(hasher, right);
        }
        ExprKind::Cast(inner, _)
        | ExprKind::Type(inner, _)
        | ExprKind::Ret(Some(inner))
        | ExprKind::Break(_, Some(inner))
        | ExprKind::Repeat(inner, _)
        | ExprKind::DropTemps(inner)
        | ExprKind::Yield(inner, _)
        | ExprKind::Become(inner) => hash_expr(hasher, inner),
        ExprKind::Struct(_, fields, _) => {
            fields.len().hash(hasher);
            for field in *fields {
                hash_expr(hasher, field.expr);
            }
        }
        ExprKind::Tup(exprs) | ExprKind::Array(exprs) => {
            exprs.len().hash(hasher);
            for expr in *exprs {
                hash_expr(hasher, expr);
            }
        }
        ExprKind::Let(let_expr) => hash_expr(hasher, let_expr.init),
        _ => {}
    }
}

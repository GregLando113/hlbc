//! The decompiler used to get haxe sources back from the bytecode definitions.
//! More info on how everything works in the [wiki](https://github.com/Gui-Yom/hlbc/wiki/Decompilation).
//!
//! The decompiler takes bytecode elements as input and outputs [ast] structures that can be displayed.

use std::collections::{HashMap, HashSet};

use ast::*;
use hlbc::opcodes::Opcode;
use hlbc::types::{Function, RefField, RefFun, Reg, Type, TypeObj};
use hlbc::Bytecode;
use scopes::*;

#[cfg(feature = "alt")]
mod alt;
/// A simple representation for the Haxe source code generated by the decompiler
pub mod ast;
/// Functions to render the [ast] to a string
pub mod fmt;
/// AST post-processing
mod post;
/// Scope handling structures
mod scopes;

enum ExprCtx {
    Constructor {
        reg: Reg,
        pos: usize,
    },
    Anonymous {
        pos: usize,
        fields: HashMap<RefField, Expr>,
        remaining: usize,
    },
}

struct DecompilerState<'c> {
    // Scope stack, holds the statements
    scopes: Scopes,
    // Expression values for each registers
    reg_state: HashMap<Reg, Expr>,
    // For parsing statements made of multiple instructions like constructor calls and anonymous structures
    // TODO move this to another pass on the generated ast
    expr_ctx: Vec<ExprCtx>,
    // Variable names we already declared
    seen: HashSet<String>,
    f: &'c Function,
    code: &'c Bytecode,
}

impl<'c> DecompilerState<'c> {
    fn new(code: &'c Bytecode, f: &'c Function) -> DecompilerState<'c> {
        let scopes = Scopes::new();
        let mut reg_state = HashMap::with_capacity(f.regs.len());
        let expr_ctx = Vec::new();
        let mut seen = HashSet::new();

        let mut start = 0;
        // First argument / First register is 'this'
        if f.is_method()
            || f.name
                .map(|n| n.resolve(&code.strings) == "__constructor__")
                .unwrap_or(false)
        {
            reg_state.insert(Reg(0), cst_this());
            start = 1;
        }

        // Initialize register state with the function arguments
        for i in start..f.ty(code).args.len() {
            let name = f.arg_name(code, i - start).map(ToOwned::to_owned);
            reg_state.insert(Reg(i as u32), Expr::Variable(Reg(i as u32), name.clone()));
            if let Some(name) = name {
                seen.insert(name);
            }
        }

        Self {
            scopes,
            reg_state,
            expr_ctx,
            seen,
            f,
            code,
        }
    }

    fn push_stmt(&mut self, stmt: Statement) {
        self.scopes.push_stmt(stmt);
    }

    // Update the register state and create a statement depending on inline rules
    fn push_expr(&mut self, i: usize, dst: Reg, expr: Expr) {
        let name = self.f.var_name(self.code, i);
        // Inline check
        if name.is_none() {
            self.reg_state.insert(dst, expr);
        } else {
            self.reg_state
                .insert(dst, Expr::Variable(dst, name.clone()));
            let declaration = self.seen.insert(name.clone().unwrap());
            self.push_stmt(Statement::Assign {
                declaration,
                variable: Expr::Variable(dst, name),
                assign: expr,
            });
        }
    }

    // Get the expr for a register
    fn expr(&self, reg: Reg) -> Expr {
        self.reg_state
            .get(&reg)
            .cloned()
            .unwrap_or_else(|| Expr::Unknown("missing expr".to_owned()))
    }

    /// Expands the expression of many registers
    fn args_expr(&self, args: &[Reg]) -> Vec<Expr> {
        args.iter().map(|&r| self.expr(r)).collect()
    }

    /// Push a call to a function, which might be a constructor call.
    fn push_call(&mut self, i: usize, dst: Reg, fun: RefFun, args: &[Reg]) {
        if let Some(&ExprCtx::Constructor { reg, pos }) = self.expr_ctx.last() {
            if reg == args[0] {
                self.push_expr(
                    pos,
                    reg,
                    Expr::Constructor(ConstructorCall::new(
                        self.f.regtype(reg),
                        self.args_expr(&args[1..]),
                    )),
                );
                self.expr_ctx.pop();
            }
        } else {
            self.push_stmt(comment(fun.display_id(self.code).to_string()));
            let call = if let Some((func, true)) = fun
                .resolve_as_fn(self.code)
                .map(|func| (func, func.is_method()))
            {
                call(
                    Expr::Field(
                        Box::new(self.expr(args[0])),
                        func.name.unwrap().resolve(&self.code.strings).to_owned(),
                    ),
                    self.args_expr(&args[1..]),
                )
            } else {
                call_fun(fun, self.args_expr(args))
            };
            if fun.ty(self.code).ret.is_void() {
                self.push_stmt(stmt(call));
            } else {
                self.push_expr(i, dst, call);
            }
        }
    }

    /// Process a jmp instruction, might be the exit condition of a loop or an if
    fn push_jmp(&mut self, i: usize, offset: i32, cond: Expr) {
        if offset > 0 {
            // It's a loop
            if matches!(self.f.ops[i + offset as usize], Opcode::JAlways { offset } if offset < 0) {
                if let Some(loop_cond) = self.scopes.last_loop_cond_mut() {
                    if matches!(loop_cond, Expr::Unknown(_)) {
                        println!("old loop cond : {:?}", loop_cond);
                        *loop_cond = cond;
                    } else {
                        self.scopes.push_if(offset + 1, cond);
                    }
                } else {
                    self.scopes.push_if(offset + 1, cond);
                }
            } else {
                // It's an if
                self.scopes.push_if(offset + 1, cond);
            }
        }
    }
}

/// Decompile a function code to a list of [Statement]s.
/// This works by analyzing each opcodes in order while trying to reconstruct scopes, contexts and intents.
pub fn decompile_code(code: &Bytecode, f: &Function) -> Vec<Statement> {
    let mut state = DecompilerState::new(code, f);

    let iter = f.ops.iter().enumerate();
    for (i, o) in iter {
        // Opcodes are grouped by semantic
        // Control flow first because they are the most important
        match o {
            //region CONTROL FLOW
            &Opcode::JTrue { cond, offset } => state.push_jmp(i, offset, not(state.expr(cond))),
            &Opcode::JFalse { cond, offset } => state.push_jmp(i, offset, state.expr(cond)),
            &Opcode::JNull { reg, offset } => {
                state.push_jmp(i, offset, noteq(state.expr(reg), cst_null()))
            }
            &Opcode::JNotNull { reg, offset } => {
                state.push_jmp(i, offset, eq(state.expr(reg), cst_null()))
            }
            &Opcode::JSGte { a, b, offset } | &Opcode::JUGte { a, b, offset } => {
                state.push_jmp(i, offset, gt(state.expr(b), state.expr(a)))
            }
            &Opcode::JSGt { a, b, offset } => {
                state.push_jmp(i, offset, gte(state.expr(b), state.expr(a)))
            }
            &Opcode::JSLte { a, b, offset } => {
                state.push_jmp(i, offset, lt(state.expr(b), state.expr(a)))
            }
            &Opcode::JSLt { a, b, offset } | &Opcode::JULt { a, b, offset } => {
                state.push_jmp(i, offset, lte(state.expr(b), state.expr(a)))
            }
            &Opcode::JEq { a, b, offset } => {
                state.push_jmp(i, offset, noteq(state.expr(a), state.expr(b)))
            }
            &Opcode::JNotEq { a, b, offset } => {
                state.push_jmp(i, offset, eq(state.expr(a), state.expr(b)))
            }
            // Unconditional jumps can actually mean a lot of things
            &Opcode::JAlways { offset } => {
                if offset < 0 {
                    // It's either the jump backward of a loop or a continue statement
                    let loop_start = state
                        .scopes
                        .last_loop_start()
                        .expect("Backward jump but we aren't in a loop ?");

                    // Scan the next instructions in order to find another jump to the same place
                    if f.ops.iter().enumerate().skip(i + 1).find_map(|(j, o)| {
                        // We found another jump to the same place !
                        if matches!(o, Opcode::JAlways {offset} if (j as i32 + offset + 1) as usize == loop_start) {
                            Some(true)
                        } else {
                            None
                        }
                    }).unwrap_or(false) {
                        // If this jump is not the last jump backward for the current loop, so it's definitely a continue; statement
                        state.push_stmt(Statement::Continue);
                    } else {
                        // It's the last jump backward of the loop, which means the end of the loop
                        // we generate the loop statement
                        if let Some(stmt) = state.scopes.end_last_loop() {
                            state.push_stmt(stmt);
                        } else {
                            panic!("Last scope is not a loop !");
                        }
                    }
                } else {
                    if let Some(offsets) = state.scopes.last_is_switch_ctx() {
                        if let Some(pos) = offsets.iter().position(|o| *o == i) {
                            state.scopes.push_switch_case(pos);
                        } else {
                            panic!("no matching offset for switch case ({i})");
                        }
                    } else if state.scopes.last_loop_start().is_some() {
                        // Check the instruction just before the jump target
                        // If it's a jump backward of a loop
                        if matches!(f.ops[(i as i32 + offset) as usize], Opcode::JAlways {offset} if offset < 0)
                        {
                            // It's a break condition
                            state.push_stmt(Statement::Break);
                        }
                    } else if state.scopes.last_is_if() {
                        // It's the jump over of an else clause
                        state.scopes.push_else(offset + 1);
                    } else {
                        eprintln!(
                            "{i}: JAlways has no matching scope (last: {:?})",
                            state.scopes.scopes.last()
                        );
                    }
                }
            }
            Opcode::Switch { reg, offsets, end } => {
                // Convert to absolute positions
                state.scopes.push_switch(
                    *end + 1,
                    state.expr(*reg),
                    offsets.iter().map(|o| i + *o as usize).collect(),
                );
                // The default switch case is implicit
            }
            &Opcode::Label => state.scopes.push_loop(i),
            &Opcode::Ret { ret } => {
                // Do not display return void; only in case of an early return
                if state.scopes.has_scopes() {
                    state.push_stmt(Statement::Return(if f.regtype(ret).is_void() {
                        None
                    } else {
                        Some(state.expr(ret))
                    }));
                } else if !f.regtype(ret).is_void() {
                    state.push_stmt(Statement::Return(Some(state.expr(ret))));
                }
            }
            //endregion

            //region EXCEPTIONS
            &Opcode::Throw { exc } | &Opcode::Rethrow { exc } => {
                state.push_stmt(Statement::Throw(state.expr(exc)));
            }
            &Opcode::Trap { exc, offset } => {
                state.scopes.push_try(offset + 1);
            }
            &Opcode::EndTrap { exc } => {
                // TODO try catch
            }
            //endregion

            //region CONSTANTS
            &Opcode::Int { dst, ptr } => {
                state.push_expr(i, dst, cst_int(ptr.resolve(&code.ints)));
            }
            &Opcode::Float { dst, ptr } => {
                state.push_expr(i, dst, cst_float(ptr.resolve(&code.floats)));
            }
            &Opcode::Bool { dst, value } => {
                state.push_expr(i, dst, cst_bool(value.0));
            }
            &Opcode::String { dst, ptr } => {
                state.push_expr(i, dst, cst_refstring(ptr, code));
            }
            &Opcode::Null { dst } => {
                state.push_expr(i, dst, cst_null());
            }
            //endregion

            //region OPERATORS
            &Opcode::Mov { dst, src } => {
                state.push_expr(i, dst, state.expr(src));
                // Workaround for when the instructions after this one use dst and src interchangeably.
                state
                    .reg_state
                    .insert(src, Expr::Variable(dst, f.var_name(code, i)));
            }
            &Opcode::Add { dst, a, b } => {
                state.push_expr(i, dst, add(state.expr(a), state.expr(b)));
            }
            &Opcode::Sub { dst, a, b } => {
                state.push_expr(i, dst, sub(state.expr(a), state.expr(b)));
            }
            &Opcode::Mul { dst, a, b } => {
                state.push_expr(i, dst, mul(state.expr(a), state.expr(b)));
            }
            &Opcode::SDiv { dst, a, b } | &Opcode::UDiv { dst, a, b } => {
                state.push_expr(i, dst, div(state.expr(a), state.expr(b)));
            }
            &Opcode::SMod { dst, a, b } | &Opcode::UMod { dst, a, b } => {
                state.push_expr(i, dst, modulo(state.expr(a), state.expr(b)));
            }
            &Opcode::Shl { dst, a, b } => {
                state.push_expr(i, dst, shl(state.expr(a), state.expr(b)));
            }
            &Opcode::SShr { dst, a, b } | &Opcode::UShr { dst, a, b } => {
                state.push_expr(i, dst, shr(state.expr(a), state.expr(b)));
            }
            &Opcode::And { dst, a, b } => {
                state.push_expr(i, dst, and(state.expr(a), state.expr(b)));
            }
            &Opcode::Or { dst, a, b } => {
                state.push_expr(i, dst, or(state.expr(a), state.expr(b)));
            }
            &Opcode::Xor { dst, a, b } => {
                state.push_expr(i, dst, xor(state.expr(a), state.expr(b)));
            }
            &Opcode::Neg { dst, src } => {
                state.push_expr(i, dst, neg(state.expr(src)));
            }
            &Opcode::Not { dst, src } => {
                state.push_expr(i, dst, not(state.expr(src)));
            }
            &Opcode::Incr { dst } => {
                // FIXME sometimes it should be an expression
                state.push_stmt(stmt(incr(state.expr(dst))));
            }
            &Opcode::Decr { dst } => {
                state.push_stmt(stmt(decr(state.expr(dst))));
            }
            //endregion

            //region CALLS
            &Opcode::Call0 { dst, fun } => {
                if fun.ty(code).ret.is_void() {
                    state.push_stmt(stmt(call_fun(fun, Vec::new())));
                } else {
                    state.push_expr(i, dst, call_fun(fun, Vec::new()));
                }
            }
            &Opcode::Call1 { dst, fun, arg0 } => {
                state.push_call(i, dst, fun, &[arg0]);
            }
            &Opcode::Call2 {
                dst,
                fun,
                arg0,
                arg1,
            } => {
                state.push_call(i, dst, fun, &[arg0, arg1]);
            }
            &Opcode::Call3 {
                dst,
                fun,
                arg0,
                arg1,
                arg2,
            } => {
                state.push_call(i, dst, fun, &[arg0, arg1, arg2]);
            }
            &Opcode::Call4 {
                dst,
                fun,
                arg0,
                arg1,
                arg2,
                arg3,
            } => {
                state.push_call(i, dst, fun, &[arg0, arg1, arg2, arg3]);
            }
            Opcode::CallN { dst, fun, args } => {
                if let Some(&ExprCtx::Constructor { reg, pos }) = state.expr_ctx.last() {
                    if reg == args[0] {
                        state.push_expr(
                            pos,
                            reg,
                            Expr::Constructor(ConstructorCall::new(
                                f.regtype(reg),
                                state.args_expr(&args[1..]),
                            )),
                        );
                    }
                } else {
                    state.push_stmt(comment(fun.display_id(code).to_string()));
                    let call = call_fun(*fun, state.args_expr(args));
                    if fun.ty(code).ret.is_void() {
                        state.push_stmt(stmt(call));
                    } else {
                        state.push_expr(i, *dst, call);
                    }
                }
            }
            Opcode::CallMethod { dst, field, args } => {
                let call = call(
                    ast::field(state.expr(args[0]), f.regtype(args[0]), *field, code),
                    state.args_expr(&args[1..]),
                );
                if f.regtype(args[0])
                    .method(field.0, code)
                    .and_then(|p| p.findex.resolve_as_fn(code))
                    .map(|fun| fun.ty(code).ret.is_void())
                    .unwrap_or(false)
                {
                    state.push_stmt(stmt(call));
                } else {
                    state.push_expr(i, *dst, call);
                }
            }
            Opcode::CallThis { dst, field, args } => {
                let method = f.regs[0].method(field.0, code).unwrap();
                let call = call(
                    Expr::Field(
                        Box::new(cst_this()),
                        method.name.resolve(&code.strings).to_owned(),
                    ),
                    state.args_expr(args),
                );
                if method
                    .findex
                    .resolve_as_fn(code)
                    .map(|fun| fun.ty(code).ret.is_void())
                    .unwrap_or(false)
                {
                    state.push_stmt(stmt(call));
                } else {
                    state.push_expr(i, *dst, call);
                }
            }
            Opcode::CallClosure { dst, fun, args } => {
                let call = call(state.expr(*fun), state.args_expr(args));
                if f.regtype(*fun)
                    .resolve_as_fun(&code.types)
                    .map(|ty| ty.ret.is_void())
                    .unwrap_or(false)
                {
                    state.push_stmt(stmt(call));
                } else {
                    state.push_expr(i, *dst, call);
                }
            }
            //endregion

            //region CLOSURES
            &Opcode::StaticClosure { dst, fun } => {
                state.push_stmt(comment(format!("closure : {}", fun.display_id(code))));
                state.push_expr(
                    i,
                    dst,
                    Expr::Closure(fun, decompile_code(code, fun.resolve_as_fn(code).unwrap())),
                );
            }
            &Opcode::InstanceClosure { dst, obj, fun } => {
                state.push_stmt(comment(format!("closure : {}", fun.display_id(code))));
                match f.regtype(obj).resolve(&code.types) {
                    // This is an anonymous enum holding the capture for the closure
                    Type::Enum { .. } => {
                        state.push_expr(
                            i,
                            dst,
                            Expr::Closure(
                                fun,
                                decompile_code(code, fun.resolve_as_fn(code).unwrap()),
                            ),
                        );
                    }
                    _ => {
                        state.push_expr(
                            i,
                            dst,
                            Expr::Field(
                                Box::new(state.expr(obj)),
                                fun.resolve_as_fn(code)
                                    .unwrap()
                                    .name(code)
                                    .unwrap_or("_")
                                    .to_owned(),
                            ),
                        );
                    }
                }
            }
            //endregion

            //region ACCESSES
            &Opcode::GetGlobal { dst, global } => {
                // Is a string
                if f.regtype(dst).0 == 13 {
                    state.push_expr(
                        i,
                        dst,
                        cst_string(
                            code.globals_initializers
                                .get(&global)
                                .and_then(|&x| {
                                    code.constants.as_ref().map(|constants| {
                                        code.strings[constants[x].fields[0]].to_owned()
                                    })
                                })
                                .unwrap(),
                        ),
                    );
                } else {
                    match f.regtype(dst).resolve(&code.types) {
                        Type::Obj(obj) | Type::Struct(obj) => {
                            state.push_expr(
                                i,
                                dst,
                                Expr::Variable(dst, Some(obj.name.display(code))),
                            );
                        }
                        Type::Enum { .. } => {
                            state.push_expr(
                                i,
                                dst,
                                Expr::Unknown("unknown enum variant".to_owned()),
                            );
                        }
                        _ => {}
                    }
                }
            }
            &Opcode::Field { dst, obj, field } => {
                state.push_expr(
                    i,
                    dst,
                    ast::field(state.expr(obj), f.regtype(obj), field, code),
                );
            }
            &Opcode::SetField { obj, field, src } => {
                let ctx = state.expr_ctx.pop();
                // Might be a SetField for an anonymous structure
                if let Some(ExprCtx::Anonymous {
                    pos,
                    mut fields,
                    mut remaining,
                }) = ctx
                {
                    fields.insert(field, state.expr(src));
                    remaining -= 1;
                    // If we filled all the structure fields, we emit an expr
                    if remaining == 0 {
                        state.push_expr(pos, obj, Expr::Anonymous(f.regtype(obj), fields));
                    } else {
                        state.expr_ctx.push(ExprCtx::Anonymous {
                            pos,
                            fields,
                            remaining,
                        });
                    }
                } else if let Some(ctx) = ctx {
                    state.expr_ctx.push(ctx);
                } else {
                    // Otherwise this is just a normal field set
                    state.push_stmt(Statement::Assign {
                        declaration: false,
                        variable: ast::field(state.expr(obj), f.regtype(obj), field, code),
                        assign: state.expr(src),
                    });
                }
            }
            &Opcode::GetThis { dst, field } => {
                state.push_expr(i, dst, ast::field(cst_this(), f.regs[0], field, code));
            }
            &Opcode::SetThis { field, src } => {
                state.push_stmt(Statement::Assign {
                    declaration: false,
                    variable: ast::field(cst_this(), f.regs[0], field, code),
                    assign: state.expr(src),
                });
            }
            &Opcode::DynGet { dst, obj, field } => {
                state.push_expr(i, dst, array(state.expr(obj), cst_refstring(field, code)));
            }
            &Opcode::DynSet { obj, field, src } => {
                state.push_stmt(Statement::Assign {
                    declaration: false,
                    variable: array(state.expr(obj), cst_refstring(field, code)),
                    assign: state.expr(src),
                });
            }
            //endregion

            //region VALUES
            &Opcode::ToDyn { dst, src }
            | &Opcode::ToSFloat { dst, src }
            | &Opcode::ToUFloat { dst, src }
            | &Opcode::ToInt { dst, src }
            | &Opcode::SafeCast { dst, src }
            | &Opcode::UnsafeCast { dst, src }
            | &Opcode::ToVirtual { dst, src } => {
                state.push_expr(i, dst, state.expr(src));
            }
            &Opcode::Ref { dst, src } => {
                state.push_expr(i, dst, state.expr(src));
            }
            &Opcode::Unref { dst, src } => {
                state.push_expr(i, dst, state.expr(src));
            }
            &Opcode::Setref { dst, value } => {
                state.push_stmt(Statement::Assign {
                    declaration: false,
                    variable: state.expr(dst),
                    assign: state.expr(value),
                });
            }
            &Opcode::RefData { dst, src } => {
                state.push_expr(i, dst, state.expr(src));
            }
            &Opcode::New { dst } => {
                // Constructor analysis
                let ty = f.regtype(dst).resolve(&code.types);
                match ty {
                    Type::Obj(_) | Type::Struct(_) => {
                        state
                            .expr_ctx
                            .push(ExprCtx::Constructor { reg: dst, pos: i });
                    }
                    Type::Virtual { fields } => {
                        state.expr_ctx.push(ExprCtx::Anonymous {
                            pos: i,
                            fields: HashMap::with_capacity(fields.len()),
                            remaining: fields.len(),
                        });
                    }
                    _ => {
                        state.push_expr(
                            i,
                            dst,
                            Expr::Constructor(ConstructorCall::new(f.regtype(dst), Vec::new())),
                        );
                    }
                }
            }
            //endregion

            //region ENUMS
            &Opcode::EnumAlloc { dst, construct } => {
                state.push_expr(
                    i,
                    dst,
                    Expr::EnumConstr(f.regtype(dst), construct, Vec::new()),
                );
            }
            Opcode::MakeEnum {
                dst,
                construct,
                args,
            } => {
                state.push_expr(
                    i,
                    *dst,
                    Expr::EnumConstr(f.regtype(*dst), *construct, state.args_expr(args)),
                );
            }
            &Opcode::EnumIndex { dst, value } => {
                state.push_expr(
                    i,
                    dst,
                    Expr::Field(Box::new(state.expr(value)), "constructorIndex".to_owned()),
                );
                //state.push_expr(i, dst, state.expr(value));
            }
            &Opcode::EnumField {
                dst,
                value,
                construct,
                field,
            } => {
                state.push_expr(
                    i,
                    dst,
                    Expr::Field(Box::new(state.expr(value)), field.0.to_string()),
                );
            }
            &Opcode::SetEnumField { value, field, src } => match state.expr(value) {
                Expr::Variable(r, name) => {
                    state.push_stmt(Statement::Assign {
                        declaration: false,
                        variable: Expr::Field(Box::new(state.expr(value)), field.0.to_string()),
                        assign: state.expr(src),
                    });
                }
                _ => {
                    state.push_stmt(comment("closure capture"));
                    state.push_stmt(Statement::Assign {
                        declaration: false,
                        variable: Expr::Field(Box::new(state.expr(value)), field.0.to_string()),
                        assign: state.expr(src),
                    });
                }
            },
            //endregion

            //region ARRAYS
            &Opcode::ArraySize { dst, array } => {
                state.push_expr(
                    i,
                    dst,
                    Expr::Field(Box::new(state.expr(array)), "length".to_owned()),
                );
            }
            &Opcode::GetArray { dst, array, index } => {
                state.push_expr(i, dst, ast::array(state.expr(array), state.expr(index)));
            }
            &Opcode::SetArray { array, index, src } => {
                state.push_stmt(Statement::Assign {
                    declaration: false,
                    variable: ast::array(state.expr(array), state.expr(index)),
                    assign: state.expr(src),
                });
            }
            //endregion

            //region MEM
            &Opcode::GetMem { dst, bytes, index } => {
                state.push_expr(i, dst, array(state.expr(bytes), state.expr(index)));
            }
            &Opcode::SetMem { bytes, index, src } => {
                state.push_stmt(Statement::Assign {
                    declaration: false,
                    variable: array(state.expr(bytes), state.expr(index)),
                    assign: state.expr(src),
                });
            }
            //endregion
            _ => {}
        }
        state.scopes.advance();
    }
    let mut statements = state.scopes.statements();

    // AST post processing step !
    // It makes a single pass for all visitors
    post::visit(
        code,
        &mut statements,
        &mut [
            Box::new(post::IfExpressions),
            Box::new(post::StringConcat),
            Box::new(post::Itos),
            Box::new(post::Trace),
        ],
    );

    statements
}

/// Decompile a function out of context
pub fn decompile_function(code: &Bytecode, f: &Function) -> Method {
    Method {
        fun: f.findex,
        static_: true,
        dynamic: false,
        statements: decompile_code(code, f),
    }
}

/// Decompile a class with its static and instance fields and methods.
pub fn decompile_class(code: &Bytecode, obj: &TypeObj) -> Class {
    let static_type = obj.get_static_type(code);

    let mut fields = Vec::new();
    for (i, f) in obj.own_fields.iter().enumerate() {
        if obj
            .bindings
            .get(&RefField(i + obj.fields.len() - obj.own_fields.len()))
            .is_some()
        {
            continue;
        }
        fields.push(ClassField {
            name: f.name.display(code),
            static_: false,
            ty: f.t,
        });
    }
    if let Some(ty) = static_type {
        for (i, f) in ty.own_fields.iter().enumerate() {
            if ty
                .bindings
                .get(&RefField(i + ty.fields.len() - ty.own_fields.len()))
                .is_some()
            {
                continue;
            }
            fields.push(ClassField {
                name: f.name.display(code),
                static_: true,
                ty: f.t,
            });
        }
    }

    let mut methods = Vec::new();
    for fun in obj.bindings.values() {
        methods.push(Method {
            fun: *fun,
            static_: false,
            dynamic: true,
            statements: decompile_code(code, fun.resolve_as_fn(code).unwrap()),
        })
    }
    if let Some(ty) = static_type {
        for fun in ty.bindings.values() {
            methods.push(Method {
                fun: *fun,
                static_: true,
                dynamic: false,
                statements: decompile_code(code, fun.resolve_as_fn(code).unwrap()),
            })
        }
    }
    for f in &obj.protos {
        methods.push(Method {
            fun: f.findex,
            static_: false,
            dynamic: false,
            statements: decompile_code(code, f.findex.resolve_as_fn(code).unwrap()),
        })
    }

    Class {
        name: obj.name.resolve(&code.strings).to_owned(),
        parent: obj
            .super_
            .and_then(|ty| ty.resolve_as_obj(&code.types))
            .map(|ty| ty.name.display(code)),
        fields,
        methods,
    }
}

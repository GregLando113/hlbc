use std::fmt::{Display, Formatter, Result};

use crate::opcodes::Opcode;
use crate::types::{
    FunPtr, Function, Native, RefEnumConstruct, RefField, RefFloat, RefInt, RefString, RefType,
    Reg, Type, TypeFun, TypeObj,
};
use crate::{Bytecode, RefFun};

impl Display for Reg {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "reg{}", self.0)
    }
}

impl RefInt {
    pub fn display(&self, ctx: &Bytecode) -> impl Display {
        self.resolve(&ctx.ints)
    }
}

impl RefFloat {
    pub fn display(&self, ctx: &Bytecode) -> impl Display {
        self.resolve(&ctx.floats)
    }
}

impl RefString {
    pub fn display(&self, ctx: &Bytecode) -> String {
        self.resolve(&ctx.strings).to_string()
    }
}

impl RefType {
    pub fn display(&self, ctx: &Bytecode) -> String {
        self.resolve(&ctx.types).display(ctx)
    }

    pub fn display_id(&self, ctx: &Bytecode) -> String {
        format!("{}@{}", self.resolve(&ctx.types).display(ctx), self.0)
    }

    fn display_rec(&self, ctx: &Bytecode, parents: Vec<*const Type>) -> String {
        self.resolve(&ctx.types).display_rec(ctx, parents)
    }
}

impl RefField {
    pub fn display_obj(&self, parent: &Type, ctx: &Bytecode) -> impl Display {
        if let Some(obj) = parent.get_type_obj() {
            if self.0 < obj.fields.len() {
                obj.fields[self.0].name.display(ctx)
            } else {
                format!("field{}", self.0)
            }
        } else if let Type::Virtual { fields } = parent {
            fields[self.0].name.display(ctx)
        } else {
            format!("field{}", self.0)
        }
    }
}

impl RefEnumConstruct {
    pub fn display(&self, parent: RefType, ctx: &Bytecode) -> impl Display {
        match parent.resolve(&ctx.types) {
            Type::Enum { constructs, .. } => {
                let name = &constructs[self.0].name;
                if name.0 != 0 {
                    name.display(ctx)
                } else {
                    "_".to_string()
                }
            }
            _ => "_".to_string(),
        }
    }
}

impl Type {
    pub fn display(&self, ctx: &Bytecode) -> String {
        self.display_rec(ctx, Vec::new())
    }

    fn display_rec(&self, ctx: &Bytecode, mut parents: Vec<*const Type>) -> String {
        //println!("{:#?}", self);
        if parents.contains(&(self as *const Type)) {
            return "Self".to_string();
        }
        parents.push(self as *const Type);

        fn display_type_fun(ty: &TypeFun, ctx: &Bytecode, parents: &[*const Type]) -> String {
            let args: Vec<String> = ty
                .args
                .iter()
                .map(|a| a.display_rec(ctx, parents.to_owned()))
                .collect();
            format!(
                "({}) -> ({})",
                args.join(", "),
                ty.ret.display_rec(ctx, parents.to_owned())
            )
        }

        match self {
            Type::Void => "void".to_string(),
            Type::UI8 => "i8".to_string(),
            Type::UI16 => "i16".to_string(),
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::F32 => "f32".to_string(),
            Type::F64 => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Bytes => "bytes".to_string(),
            Type::Dyn => "dynamic".to_string(),
            Type::Fun(fun) => display_type_fun(fun, ctx, &parents),
            Type::Obj(TypeObj { name, .. }) => name.display(ctx),
            Type::Array => "array".to_string(),
            Type::Type => "type".to_string(),
            Type::Ref(reftype) => {
                format!("ref<{}>", reftype.display_rec(ctx, parents.clone()))
            }
            Type::Virtual { fields } => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|a| {
                        format!(
                            "{}: {}",
                            a.name.display(ctx),
                            a.t.display_rec(ctx, parents.clone())
                        )
                    })
                    .collect();
                format!("virtual<{}>", fields.join(", "))
            }
            Type::DynObj => "dynobj".to_string(),
            Type::Abstract { name } => name.display(ctx),
            Type::Enum { name, .. } => format!(
                "enum<{}>",
                if name.0 != 0 {
                    name.display(ctx)
                } else {
                    "_".to_string()
                }
            ),
            Type::Null(reftype) => {
                format!("null<{}>", reftype.display_rec(ctx, parents.clone()))
            }
            Type::Method(fun) => display_type_fun(fun, ctx, &parents),
            Type::Struct(TypeObj { name, fields, .. }) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|a| {
                        format!(
                            "{}: {}",
                            a.name.display(ctx),
                            a.t.display_rec(ctx, parents.clone())
                        )
                    })
                    .collect();
                format!("{}<{}>", name.display(ctx), fields.join(", "))
            }
            Type::Packed(reftype) => {
                format!("packed<{}>", reftype.display_rec(ctx, parents.clone()))
            }
        }
    }
}

impl RefFun {
    pub fn display_header<'a>(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt!({ self.resolve(ctx).display_header(ctx) })
    }

    /// Display something like `{name}@{findex}` for functions and `{lib}/{name}@{findex}` for natives.
    pub fn display_id<'a>(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt!({ self.resolve(ctx).display_id(ctx) })
    }
}

impl<'a> FunPtr<'a> {
    pub fn display_header(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt! { move
            match self {
                FunPtr::Fun(fun) => {{fun.display_header(ctx)}},
                FunPtr::Native(n) => {{n.display_header(ctx)}},
            }
        }
    }

    /// Display something like `{name}@{findex}` for functions and `{lib}/{name}@{findex}` for natives.
    pub fn display_id(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt! { move
            match self {
                FunPtr::Fun(fun) => {{fun.display_id(ctx)}},
                FunPtr::Native(n) => {{n.display_id(ctx)}},
            }
        }
    }
}

impl Native {
    pub fn display_header(&self, ctx: &Bytecode) -> impl Display {
        format!(
            "fn:native {} {}",
            self.display_id(ctx),
            self.t.display_id(ctx)
        )
    }

    /// Display something like `{lib}/{name}@{findex}`
    pub fn display_id(&self, ctx: &Bytecode) -> impl Display {
        format!(
            "{}/{}@{}",
            self.lib.resolve(&ctx.strings),
            self.name.resolve(&ctx.strings),
            self.findex.0
        )
    }
}

impl Opcode {
    /// This display is an enhanced assembly view, with nice printing and added information from the context
    pub fn display(
        &self,
        ctx: &Bytecode,
        parent: &Function,
        pos: i32,
        align: usize,
    ) -> impl Display {
        macro_rules! op {
            ($($arg:tt)*) => {
                format!("{:<align$} {}", self.name(), format_args!($($arg)*))
            };
        }

        match self {
            Opcode::Mov { dst, src } => op!("{} = {src}", dst),
            Opcode::Int { dst, ptr } => op!("{dst} = {}", ptr.display(ctx)),
            Opcode::Float { dst, ptr } => op!("{dst} = {}", ptr.display(ctx)),
            Opcode::Bool { dst, value } => op!("{dst} = {}", value.0),
            Opcode::String { dst, ptr } => op!("{dst} = \"{}\"", ptr.display(ctx)),
            Opcode::Null { dst } => op!("{dst} = null"),
            Opcode::Add { dst, a, b } => op!("{dst} = {a} + {b}"),
            Opcode::Sub { dst, a, b } => op!("{dst} = {a} - {b}"),
            Opcode::Mul { dst, a, b } => op!("{dst} = {a} * {b}"),
            Opcode::SDiv { dst, a, b } => op!("{dst} = {a} / {b}"),
            Opcode::UDiv { dst, a, b } => op!("{dst} = {a} / {b}"),
            Opcode::SMod { dst, a, b } => op!("{dst} = {a} % {b}"),
            Opcode::UMod { dst, a, b } => op!("{dst} = {a} % {b}"),
            Opcode::Shl { dst, a, b } => op!("{dst} = {a} << {b}"),
            Opcode::SShr { dst, a, b } => op!("{dst} = {a} >> {b}"),
            Opcode::UShr { dst, a, b } => op!("{dst} = {a} >> {b}"),
            Opcode::And { dst, a, b } => op!("{dst} = {a} & {b}"),
            Opcode::Or { dst, a, b } => op!("{dst} = {a} | {b}"),
            Opcode::Xor { dst, a, b } => op!("{dst} = {a} ^ {b}"),
            Opcode::Neg { dst, src } => op!("{dst} = -{src}"),
            Opcode::Not { dst, src } => op!("{dst} = !{src}"),
            Opcode::Incr { dst } => op!("{dst}++"),
            Opcode::Decr { dst } => op!("{dst}--"),
            Opcode::Call0 { dst, fun } => op!("{dst} = {}()", fun.display_id(ctx)),
            Opcode::Call1 { dst, fun, arg0 } => op!("{dst} = {}({arg0})", fun.display_id(ctx)),
            Opcode::Call2 {
                dst,
                fun,
                arg0,
                arg1,
            } => op!("{dst} = {}({arg0}, {arg1})", fun.display_id(ctx)),
            Opcode::Call3 {
                dst,
                fun,
                arg0,
                arg1,
                arg2,
            } => op!("{dst} = {}({arg0}, {arg1}, {arg2})", fun.display_id(ctx)),
            Opcode::Call4 {
                dst,
                fun,
                arg0,
                arg1,
                arg2,
                arg3,
            } => op!(
                "{dst} = {}({arg0}, {arg1},{arg2}, {arg3})",
                fun.display_id(ctx)
            ),
            Opcode::CallN { dst, fun, args } => {
                let args: Vec<String> = args.iter().map(|r| format!("{}", r)).collect();
                op!("{dst} = {}({})", fun.display_id(ctx), args.join(", "))
            }
            Opcode::CallMethod { dst, field, args } => {
                let mut args = args.iter();
                let arg0 = args.next().unwrap();
                let args: Vec<String> = args.map(|r| format!("{}", r)).collect();
                op!(
                    "{dst} = {}.{}({})",
                    arg0,
                    field.display_obj(parent.regs[arg0.0 as usize].resolve(&ctx.types), ctx),
                    args.join(", ")
                )
            }
            Opcode::CallThis { dst, field, args } => {
                let args: Vec<String> = args.iter().map(|r| format!("{}", r)).collect();
                op!(
                    "{dst} = reg0.{}({})",
                    field.display_obj(parent.regs[0].resolve(&ctx.types), ctx),
                    args.join(", ")
                )
            }
            Opcode::CallClosure { dst, fun, args } => {
                let args: Vec<String> = args.iter().map(|r| format!("{}", r)).collect();
                op!("{dst} = {fun}({})", args.join(", "))
            }
            Opcode::StaticClosure { dst, fun } => {
                op!("{dst} = {}", fun.display_header(ctx))
            }
            Opcode::InstanceClosure { dst, fun, obj } => {
                op!("{dst} = {obj}.{}", fun.display_header(ctx))
            }
            Opcode::GetGlobal { dst, global } => {
                op!("{dst} = global@{}", global.0)
            }
            Opcode::SetGlobal { global, src } => {
                op!("global@{} = {src}", global.0)
            }
            Opcode::Field { dst, obj, field } => {
                op!(
                    "{dst} = {obj}.{}",
                    field.display_obj(parent.regs[obj.0 as usize].resolve(&ctx.types), ctx)
                )
            }
            Opcode::SetField { obj, field, src } => {
                op!(
                    "{obj}.{} = {src}",
                    field.display_obj(parent.regs[obj.0 as usize].resolve(&ctx.types), ctx)
                )
            }
            Opcode::GetThis { dst, field } => {
                op!(
                    "{dst} = this.{}",
                    field.display_obj(parent.regs[0].resolve(&ctx.types), ctx)
                )
            }
            Opcode::SetThis { field, src } => {
                op!(
                    "this.{} = {src}",
                    field.display_obj(parent.regs[0].resolve(&ctx.types), ctx)
                )
            }
            Opcode::DynGet { dst, obj, field } => {
                op!("{dst} = {obj}[\"{}\"]", field.resolve(&ctx.strings))
            }
            Opcode::DynSet { obj, field, src } => {
                op!("{obj}[\"{}\"] = {src}", field.resolve(&ctx.strings))
            }
            Opcode::JTrue { cond, offset } => {
                op!("if {cond} == true jump to {}", pos + offset + 1)
            }
            Opcode::JFalse { cond, offset } => {
                op!("if {cond} == false jump to {}", pos + offset + 1)
            }
            Opcode::JNull { reg, offset } => {
                op!("if {reg} == null jump to {}", pos + offset + 1)
            }
            Opcode::JNotNull { reg, offset } => {
                op!("if {reg} != null jump to {}", pos + offset + 1)
            }
            Opcode::JSLt { a, b, offset } => {
                op!("if {a} < {b} jump to {}", pos + offset + 1)
            }
            Opcode::JSGte { a, b, offset } => {
                op!("if {a} >= {b} jump to {}", pos + offset + 1)
            }
            Opcode::JSGt { a, b, offset } => {
                op!("if {a} > {b} jump to {}", pos + offset + 1)
            }
            Opcode::JSLte { a, b, offset } => {
                op!("if {a} <= {b} jump to {}", pos + offset + 1)
            }
            Opcode::JULt { a, b, offset } => {
                op!("if {a} < {b} jump to {}", pos + offset + 1)
            }
            Opcode::JUGte { a, b, offset } => {
                op!("if {a} >= {b} jump to {}", pos + offset + 1)
            }
            Opcode::JNotLt { a, b, offset } => {
                op!("if {a} !< {b} jump to {}", pos + offset + 1)
            }
            Opcode::JNotGte { a, b, offset } => {
                op!("if {a} !>= {b} jump to {}", pos + offset + 1)
            }
            Opcode::JEq { a, b, offset } => {
                op!("if {a} == {b} jump to {}", pos + offset + 1)
            }
            Opcode::JNotEq { a, b, offset } => {
                op!("if {a} != {b} jump to {}", pos + offset + 1)
            }
            Opcode::JAlways { offset } => {
                op!("jump to {}", pos + offset + 1)
            }
            Opcode::ToDyn { dst, src } => {
                op!("{dst} = cast {src}")
            }
            Opcode::ToInt { dst, src } => {
                op!("{dst} = cast {src}")
            }
            Opcode::SafeCast { dst, src } => {
                op!("{dst} = cast {src}")
            }
            Opcode::UnsafeCast { dst, src } => {
                op!("{dst} = cast {src}")
            }
            Opcode::ToVirtual { dst, src } => {
                op!("{dst} = cast {src}")
            }
            Opcode::Ret { ret } => op!("{ret}"),
            Opcode::Throw { exc } => {
                op!("throw {exc}")
            }
            Opcode::Rethrow { exc } => {
                op!("rethrow {exc}")
            }
            Opcode::NullCheck { reg } => {
                op!("if {reg} == null throw exc")
            }
            Opcode::Trap { exc, offset } => {
                op!("try {exc} jump to {}", pos + offset + 1)
            }
            Opcode::EndTrap { exc } => {
                op!("catch {exc}")
            }
            Opcode::GetArray { dst, array, index } => {
                op!("{dst} = {array}[{index}]")
            }
            Opcode::SetArray { array, index, src } => {
                op!("{array}[{index}] = {src}")
            }
            Opcode::New { dst } => {
                op!(
                    "{dst} = new {}",
                    parent.regs[dst.0 as usize].display_id(ctx)
                )
            }
            Opcode::ArraySize { dst, array } => {
                op!("{dst} = {array}.length")
            }
            Opcode::Type { dst, ty } => {
                op!("{dst} = {}", ty.display_id(ctx))
            }
            Opcode::Ref { dst, src } => {
                op!("{dst} = &{src}")
            }
            Opcode::Unref { dst, src } => {
                op!("{dst} = *{src}")
            }
            Opcode::MakeEnum {
                dst,
                construct,
                args,
            } => {
                let args: Vec<String> = args.iter().map(|r| format!("{}", r)).collect();
                op!(
                    "{dst} = variant {} ({})",
                    construct.display(parent.regs[dst.0 as usize], ctx),
                    args.join(", ")
                )
            }
            Opcode::EnumAlloc { dst, construct } => {
                op!(
                    "{dst} = new {}",
                    construct.display(parent.regs[dst.0 as usize], ctx)
                )
            }
            Opcode::EnumIndex { dst, value } => {
                op!("{dst} = variant of {value}")
            }
            Opcode::EnumField {
                dst,
                value,
                construct,
                field,
            } => {
                op!(
                    "{dst} = ({value} as {}).{}",
                    construct.display(parent.regs[dst.0 as usize], ctx),
                    field.0
                )
            }
            Opcode::SetEnumField { value, field, src } => {
                op!("{value}.{} = {src}", field.0)
            }
            _ => format!("{self:?}"),
        }
    }
}

impl Function {
    pub fn display_header<'a>(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt!("fn "{self.display_id(ctx)}" "{self.t.display_id(ctx)})
    }

    /// Display something like `{name}@{findex}`
    pub fn display_id<'a>(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt!({self.name_default(ctx)}"@"{self.findex.0})
    }

    pub fn display<'a>(&'a self, ctx: &'a Bytecode) -> impl Display + 'a {
        fmtools::fmt! {
            {self.display_header(ctx)}" ("{self.regs.len()}" regs, "{self.ops.len()}" ops)\n"
            for (i, reg) in self.regs.iter().enumerate() {
                "    reg"{i:<2}" "{reg.display_id(ctx)}"\n"
            }
            if let Some(debug) = &self.debug_info {
                for ((i, o), (file, line)) in self.ops
                    .iter()
                    .enumerate()
                    .zip(debug.iter())
                {
                    {ctx.debug_files.as_ref().unwrap()[*file as usize]:>12}":"{line:<3}" "{i:>3}": "{o.display(ctx, self, i as i32, 11)}"\n"
                }
            } else {
                for (i, o) in self.ops
                    .iter()
                    .enumerate() {
                    {i:>3}": "{o.display(ctx, self, i as i32, 11)}"\n"
                }
            }
        }
    }
}

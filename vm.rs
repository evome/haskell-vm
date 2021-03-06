use std::fmt;
use std::rc::Rc;
use std::path::Path;
use std::io::File;
use std::str::from_utf8;
use std::vec::from_fn;
use typecheck::TypeEnvironment;
use compiler::*;
use parser::Parser;    

#[deriving(Clone)]
enum Node_<'a> {
    Application(Node<'a>, Node<'a>),
    Int(int),
    Float(f64),
    Char(char),
    Combinator(&'a SuperCombinator),
    Indirection(Node<'a>),
    Constructor(u16, ~[Node<'a>]),
    Dictionary(&'a [uint])
}
#[deriving(Clone)]
struct Node<'a> {
    node: Rc<Node_<'a>>
}

impl <'a> Node<'a> {
    fn new(n : Node_<'a>) -> Node<'a> {
        Node { node: Rc::new(n) }
    }
    fn borrow<'b>(&'b self) -> &'b Node_<'a> {
        self.node.borrow()
    }
}
impl <'a> fmt::Default for Node<'a> {
    fn fmt(node: &Node<'a>, f: &mut fmt::Formatter) {
        write!(f.buf, "{}", *node.borrow())
    }
}
impl <'a, 'b> fmt::Default for &'b Node_<'a> {
    fn fmt(node: & &Node_<'a>, f: &mut fmt::Formatter) {
        write!(f.buf, "{}", **node)
    }
}

impl <'a> fmt::Default for Node_<'a> {
    fn fmt(node: &Node_<'a>, f: &mut fmt::Formatter) {
        match node {
            &Application(ref func, ref arg) => write!(f.buf, "({} {})", *func, *arg),
            &Int(i) => write!(f.buf, "{}", i),
            &Float(i) => write!(f.buf, "{}", i),
            &Char(c) => write!(f.buf, "'{}'", c),
            &Combinator(ref sc) => write!(f.buf, "{}", sc.name),
            &Indirection(ref n) => write!(f.buf, "(~> {})", *n),
            &Constructor(ref tag, ref args) => {
                let mut cons = args;
                if cons.len() > 0 {
                    match cons[0].borrow() {
                        &Char(_) => {
                            write!(f.buf, "\"");
                            //Print a string
                            loop {
                                if cons.len() < 2 {
                                    break;
                                }
                                match cons[0].borrow() {
                                    &Char(c) => write!(f.buf, "{}", c),
                                    _ => break
                                }
                                match cons[1].borrow() {
                                    &Constructor(_, ref args2) => cons = args2,
                                    _ => break
                                }
                            }
                            write!(f.buf, "\"");
                        }
                        _ => {
                            //Print a normal constructor
                            write!(f.buf, "\\{{}", *tag);
                            for arg in args.iter() {
                                write!(f.buf, " {}",arg.borrow());
                            }
                            write!(f.buf, "\\}");
                        }
                    }
                }
                else {
                    //Print a normal constructor
                    write!(f.buf, "\\{{}", *tag);
                    for arg in args.iter() {
                        write!(f.buf, " {}",arg.borrow());
                    }
                    write!(f.buf, "\\}");
                }
            }
            &Dictionary(ref dict) => write!(f.buf, "{:?}", dict)
        }
    }
}

pub struct VM<'a> {
    assembly : ~[Assembly],
    globals: ~[(uint, uint)],
    heap : ~[Node<'a>],
}

impl <'a> VM<'a> {
    pub fn new() -> VM {
        VM { assembly : ~[], heap : ~[], globals: ~[] }
    }

    ///Adds an assembly to the VM, adding entries to the global table as necessary
    pub fn add_assembly(&mut self, assembly: Assembly) {
        self.assembly.push(assembly);
        let assembly_index = self.assembly.len() - 1;
        let mut index = 0;
        for _ in self.assembly[self.assembly.len() - 1].superCombinators.iter() {
            self.globals.push((assembly_index, index));
            index += 1;
        }
    }

    pub fn evaluate(&'a self, code: &[Instruction], assembly_id: uint) -> Node_<'a> {
        let mut stack = ~[];
        self.execute(&mut stack, code, assembly_id);
        static evalCode : &'static [Instruction] = &[Eval];
        self.execute(&mut stack, evalCode, assembly_id);
        assert_eq!(stack.len(), 1);
        stack[0].borrow().clone()
    }

    pub fn execute(&'a self, stack: &mut ~[Node<'a>], code: &[Instruction], assembly_id: uint) {
        debug!("----------------------------");
        debug!("Entering frame with stack");
        for x in stack.iter() {
            debug!("{}", x.borrow());
        }
        debug!("");
        let mut i = 0;
        while i < code.len() {
            debug!("Executing instruction : {:?}", code[i]);
            match &code[i] {
                &Add => primitive(stack, |l, r| { l + r }),
                &Sub => primitive(stack, |l, r| { l - r }),
                &Multiply => primitive(stack, |l, r| { l * r }),
                &Divide => primitive(stack, |l, r| { l / r }),
                &Remainder => primitive(stack, |l, r| { l % r }),
                &IntEQ => primitive_int(stack, |l, r| { if l == r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &IntLT => primitive_int(stack, |l, r| { if l < r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &IntLE => primitive_int(stack, |l, r| { if l <= r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &IntGT => primitive_int(stack, |l, r| { if l > r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &IntGE => primitive_int(stack, |l, r| { if l >= r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &DoubleAdd => primitive_float(stack, |l, r| { Float(l + r) }),
                &DoubleSub => primitive_float(stack, |l, r| { Float(l - r) }),
                &DoubleMultiply => primitive_float(stack, |l, r| { Float(l * r) }),
                &DoubleDivide => primitive_float(stack, |l, r| { Float(l / r) }),
                &DoubleRemainder => primitive_float(stack, |l, r| { Float(l % r) }),
                &DoubleEQ => primitive_float(stack, |l, r| { if l == r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &DoubleLT => primitive_float(stack, |l, r| { if l < r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &DoubleLE => primitive_float(stack, |l, r| { if l <= r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &DoubleGT => primitive_float(stack, |l, r| { if l > r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &DoubleGE => primitive_float(stack, |l, r| { if l >= r { Constructor(0, ~[]) } else { Constructor(1, ~[]) } }),
                &IntToDouble => {
                    let top = stack.pop();
                    stack.push(match top.borrow() {
                        &Int(i) => Node::new(Float(i as f64)),
                        _ => fail!("Excpected Int in Int -> Double cast")
                    });
                }
                &DoubleToInt => {
                    let top = stack.pop();
                    stack.push(match top.borrow() {
                        &Float(f) => Node::new(Int(f as int)),
                        _ => fail!("Excpected Double in Double -> Int cast")
                    });
                }
                &PushInt(value) => { stack.push(Node::new(Int(value))); }
                &PushFloat(value) => { stack.push(Node::new(Float(value))); }
                &PushChar(value) => { stack.push(Node::new(Char(value))); }
                &Push(index) => {
                    let x = stack[index].clone();
                    debug!("Pushed {}", x.borrow());
                    for j in range(0, stack.len()) {
                        debug!(" {}  {}", j, stack[j].borrow());
                    }
                    stack.push(x);
                }
                &PushGlobal(index) => {
                    let (assembly_index, index) = self.globals[index];
                    let sc = &self.assembly[assembly_index].superCombinators[index];
                    stack.push(Node::new(Combinator(sc)));
                }
                &Mkap => {
                    assert!(stack.len() >= 2);
                    let func = stack.pop();
                    let arg = stack.pop();
                    debug!("Mkap {} {}", func.borrow(), arg.borrow());
                    stack.push(Node::new(Application(func, arg)));
                }
                &Eval => {
                    static unwindCode : &'static [Instruction] = &[Unwind];
                    let mut newStack = ~[stack.pop()];
                    self.execute(&mut newStack, unwindCode, assembly_id);
                    stack.push(newStack.pop());
                }
                &Pop(num) => {
                    for _ in range(0, num) {
                        stack.pop();
                    }
                }
                &Update(index) => {
                    stack[index] = Node::new(Indirection(stack[stack.len() - 1].clone()));
                }
                &Unwind => {
                    let x = (*stack[stack.len() - 1].borrow()).clone();
                    debug!("Unwinding {}", x);
                    match x {
                        Application(func, _) => {
                            stack.push(func);
                            i -= 1;//Redo the unwind instruction
                        }
                        Combinator(comb) => {
                            if stack.len() - 1 < comb.arity as uint {
                                while stack.len() > 1 {
                                    stack.pop();
                                }
                            }
                            else {
                                for j in range(stack.len() - (comb.arity as uint) - 1, stack.len() - 1) {
                                    stack[j] = match stack[j].borrow() {
                                        &Application(_, ref arg) => arg.clone(),
                                        _ => fail!("Expected Application")
                                    };
                                }
                                let mut newStack = ~[];
                                for i in range(0, comb.arity as uint) {
                                    let index = stack.len() - i - 2;
                                    newStack.push(stack[index].clone());
                                }
                                
                                debug!("Called {}", comb.name);
                                for j in range(0, newStack.len()) {
                                    debug!(" {}  {}", j, newStack[j].borrow());
                                }
                                self.execute(&mut newStack, comb.instructions, comb.assembly_id);
                                debug!("Returned {}", comb.name);
                                for j in range(0, newStack.len()) {
                                    debug!(" {}  {}", j, newStack[j].borrow());
                                }
                                assert_eq!(newStack.len(), 1);
                                for _ in range(0, comb.arity + 1) {
                                    stack.pop();
                                }
                                stack.push(newStack.pop());
                                i -= 1;
                            }
                        }
                        Indirection(node) => {
                            stack[stack.len() - 1] = node;
                            i -= 1;
                        }
                        _ => ()
                    }
                }
                &Slide(size) => {
                    let top = stack.pop();
                    for _ in range(0, size) {
                        stack.pop();
                    }
                    stack.push(top);
                }
                &Split(_) => {
                    let x = stack.pop();
                    match x.borrow() {
                        &Constructor(_, ref fields) => {
                            for field in fields.iter() {
                                stack.push(field.clone());
                            }
                        }
                        _ => fail!("Expected constructor in Split instruction")
                    }
                }
                &Pack(tag, arity) => {
                    let args = from_fn(arity as uint, |_| stack.pop());
                    stack.push(Node::new(Constructor(tag, args)));
                }
                &JumpFalse(address) => {
                    match stack[stack.len() - 1].borrow() {
                        &Constructor(0, _) => (),
                        &Constructor(1, _) => i = address - 1,
                        _ => ()
                    }
                    stack.pop();
                }
                &CaseJump(jump_tag) => {
                    let jumped = match stack[stack.len() - 1].borrow() {
                        &Constructor(tag, _) => {
                            if jump_tag == tag as uint {
                                i += 1;//Skip the jump instruction ie continue to the next test
                                true
                            }
                            else {
                                false
                            }
                        }
                        x => fail!("Expected constructor when executing CaseJump, got {}", x),
                    };
                    if !jumped {
                        stack.pop();
                    }
                }
                &Jump(to) => {
                    i = to - 1;
                }
                &PushDictionary(index) => {
                    let assembly = &self.assembly[assembly_id];
                    let dict : &[uint] = assembly.instance_dictionaries[index];
                    stack.push(Node::new(Dictionary(dict)));
                }
                &PushDictionaryMember(index) => {
                    let sc = {
                        let dict = match stack[0].borrow() {
                            &Dictionary(ref x) => x,
                            x => fail!("Attempted to retrieve {} as dictionary", x)
                        };
                        let gi = dict[index];
                        let (assembly_index, i) = self.globals[gi];
                        &self.assembly[assembly_index].superCombinators[i]
                    };
                    stack.push(Node::new(Combinator(sc)));
                }
                //undefined => fail!("Use of undefined instruction {:?}", undefined)
            }
            i += 1;
        }
        debug!("End frame");
        debug!("--------------------------");
    }
}

fn primitive_int(stack: &mut ~[Node], f: |int, int| -> Node_) {
    let l = stack.pop();
    let r = stack.pop();
    match (l.borrow(), r.borrow()) {
        (&Int(lhs), &Int(rhs)) => stack.push(Node::new(f(lhs, rhs))),
        (lhs, rhs) => fail!("Expected fully evaluted numbers in primitive instruction\n LHS: {}\nRHS: {} ", lhs, rhs)
    }
}
fn primitive_float(stack: &mut ~[Node], f: |f64, f64| -> Node_) {
    let l = stack.pop();
    let r = stack.pop();
    match (l.borrow(), r.borrow()) {
        (&Float(lhs), &Float(rhs)) => stack.push(Node::new(f(lhs, rhs))),
        (lhs, rhs) => fail!("Expected fully evaluted numbers in primitive instruction\n LHS: {}\nRHS: {} ", lhs, rhs)
    }
}
fn primitive(stack: &mut ~[Node], f: |int, int| -> int) {
    primitive_int(stack, |l, r| Int(f(l, r)))
}

#[deriving(Eq)]
enum VMResult {
    IntResult(int),
    DoubleResult(f64),
    ConstructorResult(u16, ~[VMResult])
}

fn compile_iter<T : Iterator<char>>(iterator: T) -> Assembly {
    let mut parser = Parser::new(iterator);
    let mut module = parser.module();
    
    let mut typer = TypeEnvironment::new();
    typer.typecheck_module(&mut module);
    
    let mut compiler = Compiler::new(&typer);
    compiler.compileModule(&module)
}

pub fn compile_file(filename: &str) -> Assembly {
    let path = &Path::new(filename);
    let s  = File::open(path).read_to_end();
    let contents : &str = from_utf8(s);
    compile_iter(contents.chars())
}

fn extract_result(node: Node_) -> Option<VMResult> {
    match node {
        Constructor(tag, fields) => {
            let mut result = ~[];
            for field in fields.iter() {
                match extract_result(field.borrow().clone()) {
                    Some(x) => result.push(x),
                    None => return None
                }
            }
            Some(ConstructorResult(tag, result))
        }
        Int(i) => Some(IntResult(i)),
        Float(i) => Some(DoubleResult(i)),
        x => {
            println!("Can't extract result {}", x);
            None
        }
    }
}

pub fn execute_main<T : Iterator<char>>(iterator: T) -> Option<VMResult> {
    let mut vm = VM::new();
    vm.add_assembly(compile_iter(iterator));
    let x = vm.assembly.iter().flat_map(|a| a.superCombinators.iter()).find(|sc| sc.name == ~"main");
    match x {
        Some(sc) => {
            assert!(sc.arity == 0);
            let result = vm.evaluate(sc.instructions, sc.assembly_id);
            extract_result(result)
        }
        None => None
    }
}

#[cfg(test)]
mod tests {

use std::path::Path;
use std::io::File;
use std::str::from_utf8;
use typecheck::TypeEnvironment;
use compiler::Compiler;
use parser::Parser;
use vm::{VM, execute_main, extract_result, IntResult, DoubleResult, ConstructorResult};

#[test]
fn test_primitive()
{
    assert_eq!(execute_main("main = primIntAdd 10 5".chars()), Some(IntResult(15)));
    assert_eq!(execute_main("main = primIntSubtract 7 (primIntMultiply 2 3)".chars()), Some(IntResult(1)));
    assert_eq!(execute_main("main = primIntDivide 10 (primIntRemainder 6 4)".chars()), Some(IntResult(5)));
    assert_eq!(execute_main("main = primDoubleDivide 3. 2.".chars()), Some(DoubleResult(1.5)));
    let s = 
r"data Bool = True | False
main = primIntLT 1 2";
    assert_eq!(execute_main(s.chars()), Some(ConstructorResult(0, ~[])));
}

#[test]
fn test_function()
{
    let module = 
r"mult2 x = primIntMultiply x 2

main = mult2 10";
    assert_eq!(execute_main(module.chars()), Some(IntResult(20)));

    let module2 = 
r"mult2 x = primIntMultiply x 2

add x y = primIntAdd y x

main = add 3 (mult2 10)";
    assert_eq!(execute_main(module2.chars()), Some(IntResult(23)));
}
#[test]
fn test_case()
{
    let module = 
r"mult2 x = primIntMultiply x 2

main = case [mult2 123, 0] of
    : x xs -> x
    [] -> 10";
    assert_eq!(execute_main(module.chars()), Some(IntResult(246)));
}

#[test]
fn test_nested_case() {
    let module = 
r"mult2 x = primIntMultiply x 2

main = case [mult2 123, 0] of
    : 246 xs -> primIntAdd 0 246
    [] -> 10";
    assert_eq!(execute_main(module.chars()), Some(IntResult(246)));
}

#[test]
fn test_nested_case2() {
    let module = 
r"mult2 x = primIntMultiply x 2

main = case [mult2 123, 0] of
    : 246 [] -> primIntAdd 0 246
    : x xs -> 20
    [] -> 10";
    assert_eq!(execute_main(module.chars()), Some(IntResult(20)));
}

#[test]
fn test_data_types()
{
    let module = 
r"data Bool = True | False

test = False

main = case test of
    False -> primIntAdd 0 0
    True -> primIntAdd 1 0";
    assert_eq!(execute_main(module.chars()), Some(IntResult(0)));
}

#[test]
fn test_typeclasses_known_types()
{
    let module = 
r"data Bool = True | False

class Test a where
    test :: a -> Int

instance Test Int where
    test x = x

instance Test Bool where
    test x = case x of
        True -> 1
        False -> 0


main = primIntSubtract (test (primIntAdd 5 0)) (test True)";
    assert_eq!(execute_main(module.chars()), Some(IntResult(4)));
}

#[test]
fn test_typeclasses_unknown()
{
    let module = 
r"data Bool = True | False

class Test a where
    test :: a -> Int

instance Test Int where
    test x = x

instance Test Bool where
    test x = case x of
        True -> 1
        False -> 0

testAdd y = primIntAdd (test (primIntAdd 5 0)) (test y)

main = testAdd True";
    assert_eq!(execute_main(module.chars()), Some(IntResult(6)));
}

#[test]
fn test_run_prelude() {
    let mut type_env = TypeEnvironment::new();
    let prelude = {
        let path = &Path::new("Prelude.hs");
        let s  = File::open(path).read_to_end();
        let contents : &str = from_utf8(s);
        let mut parser = Parser::new(contents.chars()); 
        let mut module = parser.module();
        type_env.typecheck_module(&mut module);
        let mut compiler = Compiler::new(&type_env);
        compiler.compileModule(&mut module)
    };

    let assembly = {
        let file =
r"add x y = primIntAdd x y
main = foldl add 0 [1,2,3,4]";
        let mut parser = Parser::new(file.chars());
        let mut module = parser.module();
        type_env.typecheck_module(&mut module);
        let mut compiler = Compiler::new(&type_env);
        compiler.assemblies.push(&prelude);
        compiler.compileModule(&module)
    };

    let mut vm = VM::new();
    vm.add_assembly(prelude);
    vm.add_assembly(assembly);
    let x = vm.assembly.iter().flat_map(|a| a.superCombinators.iter()).find(|sc| sc.name == ~"main");
    let result = match x {
        Some(sc) => {
            assert!(sc.arity == 0);
            let result = vm.evaluate(sc.instructions, sc.assembly_id);
            extract_result(result)
        }
        None => None
    };
    assert_eq!(result, Some(IntResult(10)));
}

#[test]
fn instance_super_class() {
    let prelude = {
        let path = &Path::new("Prelude.hs");
        let s  = File::open(path).read_to_end();
        let contents : &str = from_utf8(s);
        let mut parser = Parser::new(contents.chars()); 
        let mut module = parser.module();
        let mut type_env = TypeEnvironment::new();
        type_env.typecheck_module(&mut module);
        let mut compiler = Compiler::new(&type_env);
        compiler.compileModule(&mut module)
    };

    let assembly = {
        let file = r"main = [primIntAdd 0 1,2,3,4] == [1,2,3]";
        let mut parser = Parser::new(file.chars());
        let mut module = parser.module();
        let mut type_env = TypeEnvironment::new();
        type_env.add_types(&prelude);
        type_env.typecheck_module(&mut module);
        let mut compiler = Compiler::new(&type_env);
        compiler.assemblies.push(&prelude);
        compiler.compileModule(&module)
    };

    let mut vm = VM::new();
    vm.add_assembly(prelude);
    vm.add_assembly(assembly);
    let x = vm.assembly.iter().flat_map(|a| a.superCombinators.iter()).find(|sc| sc.name == ~"main");
    let result = match x {
        Some(sc) => {
            assert!(sc.arity == 0);
            let result = vm.evaluate(sc.instructions, sc.assembly_id);
            extract_result(result)
        }
        None => None
    };
    assert_eq!(result, Some(ConstructorResult(1, ~[])));
}

}

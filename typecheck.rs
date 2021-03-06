use std::hashmap::HashMap;
use module::{TypeVariable, TypeOperator, Identifier, Number, Rational, String, Char, Apply, Lambda, Let, Case, TypedExpr, Module, Constraint, Pattern, IdentifierPattern, NumberPattern, ConstructorPattern, Binding, Class, TypeDeclaration};
use graph::{Graph, VertexIndex, strongly_connected_components};
use std::iter::range_step;

pub use lexer::Location;
pub use module::Type;

#[cfg(test)]
use module::Alternative;

///Trait which can be implemented by types where types can be looked up by name
pub trait Types {
    fn find_type<'a>(&'a self, name: &str) -> Option<&'a Type>;
    fn find_class<'a>(&'a self, name: &str) -> Option<&'a Class>;
    fn has_instance(&self, classname: &str, typ: &Type) -> bool {
        match self.find_instance(classname, typ) {
            Some(_) => true,
            None => false
        }
    }
    fn find_instance<'a>(&'a self, classname: &str, typ: &Type) -> Option<(&'a [Constraint], &'a Type)>;
    fn each_typedeclaration(&self, |&TypeDeclaration|);
}

impl Types for Module {
    fn find_type<'a>(&'a self, name: &str) -> Option<&'a Type> {
        for bind in self.bindings.iter() {
            if bind.name.equiv(&name) {
                return Some(&bind.expression.typ);
            }
        }

        for class in self.classes.iter() {
            for decl in class.declarations.iter() {
                if decl.name.equiv(&name) {
                    return Some(&decl.typ);
                }
            }
        }
        for data in self.dataDefinitions.iter() {
            for ctor in data.constructors.iter() {
                if ctor.name.equiv(&name) {
                    return Some(&ctor.typ);
                }
            }
        }
        return None;
    }

    fn find_class<'a>(&'a self, name: &str) -> Option<&'a Class> {
        self.classes.iter().find(|class| name == class.name)
    }

    fn find_instance<'a>(&'a self, classname: &str, typ: &Type) -> Option<(&'a [Constraint], &'a Type)> {
        for instance in self.instances.iter() {
            if classname == instance.classname && &instance.typ == typ {//test name
                let c : &[Constraint] = instance.constraints;
                return Some((c, &instance.typ));
            }
        }
        None
    }

    fn each_typedeclaration(&self, func: |&TypeDeclaration|) {
        for bind in self.bindings.iter() {
            func(&bind.typeDecl);
        }

        for class in self.classes.iter() {
            for decl in class.declarations.iter() {
                func(decl);
            }
        }
    }
}

pub struct TypeEnvironment<'a> {
    assemblies: ~[&'a Types],
    namedTypes : HashMap<~str, Type>,
    types : ~[Type],
    constraints: HashMap<TypeVariable, ~[~str]>,
    instances: ~[(~str, Type)],
    variableIndex : TypeVariable
}

struct TypeScope<'a, 'b> {
    vars: ~[(~str, Type)],
    env: &'a mut TypeEnvironment<'b>,
    parent: Option<&'a TypeScope<'a, 'b>>,
    non_generic: ~[Type]
}

#[deriving(Clone)]
struct Substitution {
    subs: HashMap<TypeVariable, Type>,
    constraints: HashMap<TypeVariable, ~[~str]>
}

///Signals that a type error has occured and the top level types as well as the location is needed
condition! {
    type_error: () -> (Location, Type, Type);
}


trait Bindings {
    fn get_mut<'a>(&'a mut self, idx: (uint, uint)) -> &'a mut Binding;

    fn each_binding(&self, |&Binding, (uint, uint)|);
}

impl Bindings for Module {
    fn get_mut<'a>(&'a mut self, (instance_idx, idx): (uint, uint)) -> &'a mut Binding {
        if instance_idx == 0 {
            &mut self.bindings[idx]
        }
        else {
            &mut self.instances[instance_idx - 1].bindings[idx]
        }
    }

    fn each_binding(&self, func: |&Binding, (uint, uint)|) {
        for (index, bind) in self.bindings.iter().enumerate() {
            func(bind, (0, index));
        }
        for (instance_index, instance) in self.instances.iter().enumerate() {
            for (index, bind) in instance.bindings.iter().enumerate() {
                func(bind, (instance_index + 1, index));
            }
        }
    }
}

//Woraround since traits around a vector seems problematic
struct BindingsWrapper<'a> {
    value: &'a mut [Binding]
}

impl <'a> Bindings for BindingsWrapper<'a> {
    fn get_mut<'a>(&'a mut self, (_, idx): (uint, uint)) -> &'a mut Binding {
        &mut self.value[idx]
    }

    fn each_binding(&self, func: |&Binding, (uint, uint)|) {
        for (index, bind) in self.value.iter().enumerate() {
            func(bind, (0, index));
        }
    }
}

fn add_primitives(globals: &mut HashMap<~str, Type>, typename: &str) {
    let typ = Type::new_op(typename.to_owned(), ~[]);
    {
        let binop = function_type(&typ, &function_type(&typ, &typ));
        globals.insert("prim" + typename + "Add", binop.clone());
        globals.insert("prim" + typename + "Subtract", binop.clone());
        globals.insert("prim" + typename + "Multiply", binop.clone());
        globals.insert("prim" + typename + "Divide", binop.clone());
        globals.insert("prim" + typename + "Remainder", binop.clone());
    }
    {
        let binop = function_type(&typ, &function_type(&typ, &Type::new_op(~"Bool", ~[])));
        globals.insert("prim" + typename + "EQ", binop.clone());
        globals.insert("prim" + typename + "LT", binop.clone());
        globals.insert("prim" + typename + "LE", binop.clone());
        globals.insert("prim" + typename + "GT", binop.clone());
        globals.insert("prim" + typename + "GE", binop.clone());
    }
}

fn create_tuple_type(size: uint) -> (~str, Type) {
    let var_list = ::std::vec::from_fn(size, |i| Type::new_var(i as int));
    let mut ident = ~"(";
    for _ in range(1, size) {
        ident.push_char(',');
    }
    ident.push_char(')');
    let mut typ = Type::new_op(ident.clone(), var_list);
    for i in range_step(size as int - 1, -1, -1) {
        typ = Type::new_op(~"->", ~[Type::new_var(i), typ]);
    }
    (ident, typ)
}

impl <'a> TypeEnvironment<'a> {

    ///Creates a new TypeEnvironment and adds all the primitive types
    pub fn new() -> TypeEnvironment {
        let mut globals = HashMap::new();
        add_primitives(&mut globals, &"Int");
        add_primitives(&mut globals, &"Double");
        globals.insert(~"primIntToDouble", function_type(&Type::new_op(~"Int", ~[]), &Type::new_op(~"Double", ~[])));
        globals.insert(~"primDoubleToInt", function_type(&Type::new_op(~"Double", ~[]), &Type::new_op(~"Int", ~[])));
        let var = Type::new_var(-10);
        let list = Type::new_op(~"[]", ~[var.clone()]);
        globals.insert(~"[]", list.clone());
        globals.insert(~":", function_type(&var, &function_type(&list, &list)));
        for i in range(0 as uint, 10) {
            let (name, typ) = create_tuple_type(i);
            globals.insert(name, typ);
        }
        TypeEnvironment {
            assemblies: ~[],
            namedTypes : globals,
            types : ~[] ,
            constraints: HashMap::new(),
            instances: ~[],
            variableIndex : TypeVariable { id : 0 } }
    }

    pub fn add_types(&'a mut self, types: &'a Types) {
        let mut max_id = 0;
        types.each_typedeclaration(|decl| {
            for constraint in decl.context.iter() {
                let var = constraint.variables[0].clone();
                max_id = ::std::cmp::max(var.id, max_id);
                self.constraints.find_or_insert(var, ~[]).push(constraint.class.clone());
            }
        });
        self.variableIndex.id = max_id;
        self.assemblies.push(types);
    }

    ///Typechecks a module by updating all the types in place
    pub fn typecheck_module(&mut self, module: &mut Module) {
        for data_def in module.dataDefinitions.mut_iter() {
            let mut subs = Substitution { subs: HashMap::new(), constraints: HashMap::new() };
            {
                let scope = TypeScope { env: self, vars: ~[], non_generic: ~[], parent: None };
                freshen(&scope, &mut subs.subs, &mut data_def.typ);
            }
            for constructor in data_def.constructors.mut_iter() {
                replace(&mut self.constraints, &mut constructor.typ, &subs);
                self.namedTypes.insert(constructor.name.clone(), constructor.typ.clone());
            }
        }
        for class in module.classes.mut_iter() {
            //Instantiate a new variable and replace all occurances of the class variable with this
            let replaced = class.variable.clone();
            let new = self.new_var();
            class.variable = new.var().clone();
            self.constraints.insert(class.variable.clone(), ~[class.name.clone()]);

            for type_decl in class.declarations.mut_iter() {
                let c = Constraint { class: class.name.clone(), variables: ~[class.variable.clone()] };
                let mut mapping = HashMap::new();
                mapping.insert(replaced.clone(), new.clone());
                self.freshen_declaration2(type_decl, mapping);
                type_decl.context.push(c);
                self.namedTypes.insert(type_decl.name.clone(), type_decl.typ.clone());
            }
        }
        for instance in module.instances.mut_iter() {
            let class = module.classes.iter().find(|class| class.name == instance.classname)
                .expect(format!("Could not find class {}", instance.classname));
            {
                let mut mapping = HashMap::new();
                for constraint in instance.constraints.mut_iter() {
                    let new = mapping.find_or_insert(constraint.variables[0].clone(), self.new_var());
                    constraint.variables[0] = new.var().clone();
                }
                let mut scope = TypeScope { env: self, vars: ~[], non_generic: ~[], parent: None };
                instance.typ = freshen(&mut scope, &mut mapping, &instance.typ);
            }
            for binding in instance.bindings.mut_iter() {
                let decl = class.declarations.iter().find(|decl| binding.name.ends_with(decl.name))
                    .expect(format!("Could not find {} in class {}", binding.name, class.name));
                binding.typeDecl = decl.clone();
                replace_var(&mut binding.typeDecl.typ, &class.variable, &instance.typ);
                for constraint in instance.constraints.iter() {
                    binding.typeDecl.context.push(constraint.clone());
                }
                self.freshen_declaration(&mut binding.typeDecl);
                for constraint in binding.typeDecl.context.iter() {
                    self.constraints.find_or_insert(constraint.variables[0].clone(), ~[])
                        .push(constraint.class.clone());
                }
            }
            self.instances.push((instance.classname.clone(), instance.typ.clone()));
        }
        
        for type_decl in module.typeDeclarations.mut_iter() {
            self.freshen_declaration(type_decl);

            match module.bindings.mut_iter().find(|bind| bind.name == type_decl.name) {
                Some(bind) => {
                    bind.typeDecl = type_decl.clone();
                }
                None => fail!("Error: Type declaration for '{}' has no binding", type_decl.name)
            }
        }

        {
            let mut scope = TypeScope { env: self, vars: ~[], non_generic: ~[], parent: None };
            let mut subs = Substitution { subs: HashMap::new(), constraints: HashMap::new() }; 
            scope.typecheck_mutually_recursive_bindings(&mut subs, module);
        }
        for bind in module.bindings.iter() {
            self.namedTypes.insert(bind.name.clone(), bind.expression.typ.clone());
        }
    }

    pub fn typecheck(&mut self, expr : &mut TypedExpr) {
        let mut subs = Substitution { subs: HashMap::new(), constraints: HashMap::new() }; 
        {
            let mut scope = TypeScope { env: self, vars: ~[], non_generic: ~[], parent: None };
            scope.typecheck(expr, &mut subs);
        }
        self.substitute(&mut subs, expr);
    }

    pub fn find(&'a self, ident: &str) -> Option<&'a Type> {
        self.namedTypes.find_equiv(&ident).or_else(|| {
            for types in self.assemblies.iter() {
                let v = types.find_type(ident);
                if v != None {
                    return v;
                }
            }
            None
        })
    }

    ///Finds all the constraints for a type
    pub fn find_constraints(&self, typ: &Type) -> ~[Constraint] {
        let mut result : ~[Constraint] = ~[];
        each_type(typ,
        |var| {
            match self.constraints.find(var) {
                Some(constraints) => {
                    for c in constraints.iter() {
                        if result.iter().find(|x| x.class == *c) == None {
                            result.push(Constraint { class: c.clone(), variables: ~[var.clone()] });
                        }
                    }
                }
                None => ()
            }
        },
        |_| ());
        result
    }
    
    ///Searches through a type, comparing it with the type on the identifier, returning all the specialized constraints
    pub fn find_specialized_instances(&self, name: &str, actual_type: &Type) -> ~[(~str, Type)] {
        match self.find(name) {
            Some(typ) => {
                let mut constraints = ~[];
                self.find_specialized(&mut constraints, actual_type, typ);
                constraints
            }
            None => fail!("Could not find '{}' in type environment", name)
        }
    }
    fn find_specialized(&self, constraints: &mut ~[(~str, Type)], actual_type: &Type, typ: &Type) {
        match (&actual_type.typ, &typ.typ) {
            (&TypeOperator(_), &TypeVariable(ref var)) => {
                match self.constraints.find(var) {
                    Some(cons) => {
                        for c in cons.iter() {
                            if constraints.iter().find(|x| x.n0_ref() == c) == None {
                                constraints.push((c.clone(), actual_type.clone()));
                            }
                        }
                    }
                    None => ()
                }
            }
            _ => ()
        }
        for ii in range(0, actual_type.types.len()) {
            self.find_specialized(constraints, &actual_type.types[ii], &typ.types[ii]);
        }
    }

    fn freshen_declaration2(&mut self, decl: &mut TypeDeclaration, mut mapping: HashMap<TypeVariable, Type>) {
        for constraint in decl.context.mut_iter() {
            let old = constraint.variables[0].clone();
            let new = mapping.find_or_insert(old.clone(), self.new_var());
            constraint.variables[0] = new.var().clone();
        }
        let mut scope = TypeScope { env: self, vars: ~[], non_generic: ~[], parent: None };
        decl.typ = freshen(&mut scope, &mut mapping, &decl.typ);
    }
    fn freshen_declaration(&mut self, decl: &mut TypeDeclaration) {
        let mapping = HashMap::new();
        self.freshen_declaration2(decl, mapping);
    }

    ///Applies a substitution on all global types
    fn apply(&mut self, subs: &Substitution) {
        for (_, typ) in self.namedTypes.mut_iter() {
            replace(&mut self.constraints, typ, subs);
        }
    }

    ///Walks through an expression and applies the substitution on each of its types
    fn substitute(&mut self, subs : &Substitution, expr: &mut TypedExpr) {
        replace(&mut self.constraints, &mut expr.typ, subs);
        match &mut expr.expr {
            &Apply(ref mut func, ref mut arg) => {
                self.substitute(subs, *func);
                self.substitute(subs, *arg);
            }
            &Let(ref mut bindings, ref mut let_expr) => {
                for bind in bindings.mut_iter() {
                    self.substitute(subs, &mut bind.expression);
                }
                self.substitute(subs, *let_expr);
            }
            &Case(ref mut case_expr, ref mut alts) => {
                self.substitute(subs, *case_expr);
                for alt in alts.mut_iter() {
                    self.substitute(subs, &mut alt.expression);
                }
            }
            &Lambda(_, ref mut body) => self.substitute(subs, *body),
            _ => ()
        }
    }

    ///Returns whether the type 'op' has an instance for 'class'
    fn has_instance(&self, class: &str, searched_type: &Type) -> bool {
        for &(ref name, ref typ) in self.instances.iter() {
            if class == *name && typ.typ == searched_type.typ {
                return true;
            }
        }
        
        for types in self.assemblies.iter() {
            match types.find_instance(class, searched_type) {
                Some((constraints, unspecialized_type)) => {
                    return self.check_instance_constraints(constraints, unspecialized_type.types, searched_type.types);
                }
                None => ()
            }
        }
        false
    }

    fn check_instance_constraints(&self, constraints: &[Constraint], vars: &[Type], types: &[Type]) -> bool {
        for constraint in constraints.iter() {
            //Constraint is such as (Eq a, Eq b) => Eq (Either a b)
            //Find the position in the types vector
            let variable = &constraint.variables[0];
            let maybe_pos = vars.iter().position(|typ| {
                match &typ.typ {
                    &TypeVariable(ref var) => var == variable,
                    _ => false
                }
            });
            match maybe_pos {
                Some(pos) => {
                    if !self.has_instance(constraint.class, &types[pos]) {
                        return false;
                    }
                }
                None => ()
            }
        }
        return true;
    }

    fn new_var(&mut self) -> Type {
        self.variableIndex.id += 1;
        Type::new_var(self.variableIndex.id)
    }
}
#[unsafe_destructor]
impl <'a, 'b> Drop for TypeScope<'a, 'b> {
    fn drop(&mut self) {
        while self.vars.len() > 0 {
            let (name, typ) = self.vars.pop();
            self.env.namedTypes.insert(name, typ);
        }
    }
}

impl <'a, 'b> TypeScope<'a, 'b> {

    fn apply(&mut self, subs: &Substitution) {
        self.env.apply(subs)
    }

    fn typecheck(&mut self, expr : &mut TypedExpr, subs: &mut Substitution) {
        if expr.typ == Type::new_var(0) {
            expr.typ = self.env.new_var();
        }

        match &mut expr.expr {
            &Number(_) => {
                self.env.constraints.insert(expr.typ.var().clone(), ~[~"Num"]);
            }
            &Rational(_) => {
                self.env.constraints.insert(expr.typ.var().clone(), ~[~"Fractional"]);
            }
            &String(_) => {
                expr.typ = Type::new_op(~"[]", ~[Type::new_op(~"Char", ~[])]);
            }
            &Char(_) => {
                expr.typ = Type::new_op(~"Char", ~[]);
            }
            &Identifier(ref name) => {
                match self.fresh(*name) {
                    Some(t) => {
                        expr.typ = t;
                    }
                    None => fail!("Undefined identifier '{}' at {}", *name, expr.location)
                }
            }
            &Apply(ref mut func, ref mut arg) => {
                self.typecheck(*func, subs);
                replace(&mut self.env.constraints, &mut func.typ, subs);
                self.typecheck(*arg, subs);
                replace(&mut self.env.constraints, &mut arg.typ, subs);
                expr.typ = function_type(&arg.typ, &self.env.new_var());
                unify_location(self.env, subs, &expr.location, &mut func.typ, &mut expr.typ);
                replace(&mut self.env.constraints, &mut expr.typ, subs);
                expr.typ = expr.typ.types[1].clone();
            }
            &Lambda(ref arg, ref mut body) => {
                let argType = self.env.new_var();
                expr.typ = function_type(&argType, &self.env.new_var());
                {
                    let mut childScope = self.child();
                    childScope.insert(arg.clone(), &argType);
                    childScope.non_generic.push(argType.clone());
                    childScope.typecheck(*body, subs);
                }
                replace(&mut self.env.constraints, &mut expr.typ, subs);
                expr.typ.types[1] = body.typ.clone();
            }
            &Let(ref mut bindings, ref mut body) => {
                {
                    let mut childScope = self.child();
                    childScope.typecheck_mutually_recursive_bindings(subs, &mut BindingsWrapper { value: *bindings });
                    childScope.apply(subs);
                    childScope.typecheck(*body, subs);
                }
                replace(&mut self.env.constraints, &mut body.typ, subs);
                expr.typ = body.typ.clone();
            }
            &Case(ref mut case_expr, ref mut alts) => {
                self.typecheck(*case_expr, subs);
                self.typecheck_pattern(&alts[0].pattern.location, subs, &alts[0].pattern.node, &mut case_expr.typ);
                self.typecheck(&mut alts[0].expression, subs);
                let mut alt0_ = alts[0].expression.typ.clone();
                for alt in alts.mut_iter().skip(1) {
                    self.typecheck_pattern(&alt.pattern.location, subs, &alt.pattern.node, &mut case_expr.typ);
                    self.typecheck(&mut alt.expression, subs);
                    unify_location(self.env, subs, &alt.expression.location, &mut alt0_, &mut alt.expression.typ);
                    replace(&mut self.env.constraints, &mut alt.expression.typ, subs);
                }
                replace(&mut self.env.constraints, &mut alts[0].expression.typ, subs);
                replace(&mut self.env.constraints, &mut case_expr.typ, subs);
                expr.typ = alt0_;
            }
        };
    }

    fn typecheck_pattern(&mut self, location: &Location, subs: &mut Substitution, pattern: &Pattern, match_type: &mut Type) {
        match pattern {
            &IdentifierPattern(ref ident) => {
                let mut typ = self.env.new_var();
                {
                    unify_location(self.env, subs, location, &mut typ, match_type);
                    replace(&mut self.env.constraints, match_type, subs);
                    replace(&mut self.env.constraints, &mut typ, subs);
                }
                self.insert(ident.clone(), &typ);
                self.non_generic.push(typ);
            }
            &NumberPattern(_) => {
                let mut typ = Type::new_op(~"Int", ~[]);
                {
                    unify_location(self.env, subs, location, &mut typ, match_type);
                    replace(&mut self.env.constraints, match_type, subs);
                    replace(&mut self.env.constraints, &mut typ, subs);
                }
            }
            &ConstructorPattern(ref ctorname, ref patterns) => {
                let mut t = self.fresh(*ctorname).expect(format!("Undefined constructer '{}' when matching pattern", *ctorname));
                let mut data_type = get_returntype(&t);
                
                unify_location(self.env, subs, location, &mut data_type, match_type);
                replace(&mut self.env.constraints, match_type, subs);
                replace(&mut self.env.constraints, &mut t, subs);
                self.env.apply(subs);
                self.pattern_rec(0, location, subs, *patterns, &mut t);
            }
        }
    }

    fn pattern_rec(&mut self, i: uint, location: &Location, subs: &mut Substitution, patterns: &[Pattern], func_type: &mut Type) {
        if i < patterns.len() {
            let p = &patterns[i];
            self.typecheck_pattern(location, subs, p, &mut func_type.types[0]);
            self.pattern_rec(i + 1, location, subs, patterns, &mut func_type.types[1]);
        }
    }

    pub fn typecheck_mutually_recursive_bindings(&mut self, subs: &mut Substitution, bindings: &mut Bindings) {
        
        let graph = build_graph(bindings);
        let groups = strongly_connected_components(&graph);

        for i in range(0, groups.len()) {
            let group = &groups[i];
            for index in group.iter() {
                let bindIndex = graph.get_vertex(*index).value;
                let bind = bindings.get_mut(bindIndex);
                bind.expression.typ = self.env.new_var();
                self.insert(bind.name.clone(), &bind.expression.typ);
                if bind.typeDecl.typ == Type::new_var(0) {
                    bind.typeDecl.typ = self.env.new_var();
                }
            }
            
            for index in group.iter() {
                {
                    let bindIndex = graph.get_vertex(*index).value;
                    let bind = bindings.get_mut(bindIndex);
                    debug!("Begin typecheck {} :: {}", bind.name, bind.expression.typ);
                    self.non_generic.push(bind.expression.typ.clone());
                    let type_var = bind.expression.typ.var().clone();
                    self.typecheck(&mut bind.expression, subs);
                    unify_location(self.env, subs, &bind.expression.location, &mut bind.typeDecl.typ, &mut bind.expression.typ);
                    self.env.substitute(subs, &mut bind.expression);
                    subs.subs.insert(type_var, bind.expression.typ.clone());
                    self.apply(subs);
                    debug!("End typecheck {} :: {}", bind.name, bind.expression.typ);
                }
            }
            
            for index in group.iter() {
                let bindIndex = graph.get_vertex(*index).value;
                let bind = bindings.get_mut(bindIndex);
                self.non_generic.pop();
                self.env.substitute(subs, &mut bind.expression);
                bind.typeDecl.typ = bind.expression.typ.clone();
                bind.typeDecl.context = self.env.find_constraints(&bind.typeDecl.typ);
            }
        }
    }

    fn insert(&mut self, name: ~str, t : &Type) {
        match self.env.namedTypes.pop(&name) {
            Some(typ) => self.vars.push((name.clone(), typ)),
            None => ()
        }
        self.env.namedTypes.insert(name, t.clone());
    }
    fn find(&'a self, name: &str) -> Option<&'a Type> {
        self.env.find(name)
    }

    ///Instantiates new typevariables for every typevariable in the type found at 'name'
    fn fresh(&'a self, name: &str) -> Option<Type> {
        match self.find(name) {
            Some(x) => {
                let mut mapping = HashMap::new();
                let typ = x;
                Some(freshen(self, &mut mapping, typ))
            }
            None => None
        }
    }

    fn is_generic(&'a self, var: &TypeVariable) -> bool {
        let found = self.non_generic.iter().any(|t| {
            let typ = t;
            occurs(var, typ)
        });
        if found {
            false
        }
        else {
            match self.parent {
                Some(p) => p.is_generic(var),
                None => true
            }
        }
    }

    fn child(&'a self) -> TypeScope<'a, 'b> {
        TypeScope { env: self.env, vars: ~[], non_generic: ~[], parent: Some(self) }
    }
}

fn replace_var(typ: &mut Type, var: &TypeVariable, replacement: &Type) {
    let new = match &mut typ.typ {
        &TypeVariable(ref v) => {
            if v == var {
                Some(replacement)
            }
            else {
                None
            }
        }
        &TypeOperator(_) => None
    };
    match new {
        Some(x) => {
            let is_var = match &typ.typ {
                &TypeVariable(_) => true,
                &TypeOperator(_) => false
            };
            if typ.types.len() > 0 && is_var {
                typ.typ = x.typ.clone();
            }
            else {
                *typ = x.clone();
            }
        }
        None => ()
    }
    for t in typ.types.mut_iter() {
        replace_var(t, var, replacement);
    }
}

fn get_returntype(typ: &Type) -> Type {
    match &typ.typ {
        &TypeOperator(ref op) => {
            if op.name == ~"->" {
                get_returntype(&typ.types[1])
            }
            else {
                typ.clone()
            }
        }
        _ => typ.clone()
    }
}

///Update the constraints when replacing the variable 'old' with 'new'
fn update_constraints(constraints: &mut HashMap<TypeVariable, ~[~str]>, old: &TypeVariable, new: &Type, subs: &Substitution) {
    match &new.typ {
        &TypeVariable(ref new_var) => {
            match subs.constraints.find(old) {
                Some(subs_constraints) => {
                    let to_update = constraints.find_or_insert(new_var.clone(), ~[]);
                    for c in subs_constraints.iter() {
                        if to_update.iter().find(|x| *x == c) == None {
                            to_update.push(c.clone());
                        }
                    }
                }
                None => ()
            }
        }
        _ => ()
    }
}

///Replace all typevariables using the substitution 'subs'
fn replace(constraints: &mut HashMap<TypeVariable, ~[~str]>, old : &mut Type, subs : &Substitution) {
    let replaced = match &mut old.typ {
        &TypeVariable(ref id) => {
            match subs.subs.find(id) {
                Some(new) => {
                    update_constraints(constraints, id, new, subs);
                    Some(new.clone())
                }
                None => None
            }
        }
        &TypeOperator(_) => None
    };
    match replaced {
        Some(x) => {
            let is_var = match &old.typ {
                &TypeVariable(_) => true,
                &TypeOperator(_) => false
            };
            if old.types.len() > 0 && is_var {
                old.typ = x.typ;
            }
            else {
                *old = x;
            }
        }
        None => ()
    }
    for t in old.types.mut_iter() {
        replace(constraints, t, subs); 
    }
}

///Checks whether a typevariable occurs in another type
fn occurs(type_var: &TypeVariable, inType: &Type) -> bool {
    (match &inType.typ {
        &TypeVariable(ref var) => type_var.id == var.id,
        &TypeOperator(_) => false
    }) || inType.types.iter().any(|t| occurs(type_var, t))
}

fn freshen(env: &TypeScope, mapping: &mut HashMap<TypeVariable, Type>, typ: &Type) -> Type {
    let result = match &typ.typ {
        &TypeVariable(ref id) => {
            if env.is_generic(id) {
                let new = env.env.new_var();
                let maybe_constraints = match env.env.constraints.find(id) {
                    Some(constraints) => Some(constraints.clone()),
                    None => None
                };
                match (maybe_constraints, new.typ.clone()) {
                    (Some(c), TypeVariable(newid)) => { env.env.constraints.insert(newid, c); }
                    _ => ()
                }
                mapping.find_or_insert(id.clone(), new.clone()).typ.clone()
            }
            else {
                typ.typ.clone()
            }
        }
        &TypeOperator(_) => {
            typ.typ.clone()
        }
    };
    Type { typ: result, types: typ.types.iter().map(|t| freshen(env, mapping, t)).collect() }
}

///Takes two types and attempts to make them the same type
fn unify_location(env: &mut TypeEnvironment, subs: &mut Substitution, location: &Location, lhs: &mut Type, rhs: &mut Type) {
    debug!("Unifying {} <-> {}", *lhs, *rhs);
    type_error::cond.trap(|_| (location.clone(), lhs.clone(), rhs.clone())).inside(|| {
        unify_(env, subs, lhs, rhs);
        
        let subs2 = subs.clone();
        for (_, ref mut typ) in subs.subs.mut_iter() {
            replace(&mut env.constraints, *typ, &subs2);
        }
    })
}

fn unify_(env : &mut TypeEnvironment, subs : &mut Substitution, lhs : &mut Type, rhs : &mut Type) {
    let unified = match (& &lhs.typ, & &rhs.typ) {
        (& &TypeVariable(ref lid), & &TypeVariable(ref rid)) => {
            if lid != rid {
                let mut t = Type::new_var(rid.id);
                replace(&mut env.constraints, &mut t, subs);
                subs.subs.insert(lid.clone(), t);
                match env.constraints.pop(lid) {
                    Some(constraints) => { subs.constraints.insert(lid.clone(), constraints); }
                    None => ()
                }
            }
            true
        }
        (& &TypeOperator(ref l), & &TypeOperator(ref r)) => {
            if l.name != r.name || lhs.types.len() != rhs.types.len() {
                let (location, l, r) = type_error::cond.raise(());
                fail!("{} Error: Could not unify types {}\nand\n{}", location, l, r)
            }
            for i in range(0, lhs.types.len()) {
                unify_(env, subs, &mut lhs.types[i], &mut rhs.types[i]);
                if i < lhs.types.len() - 1 {
                    replace(&mut env.constraints, &mut lhs.types[i+1], subs);
                    replace(&mut env.constraints, &mut rhs.types[i+1], subs);
                }
            }
            true
        }
        (& &TypeVariable(ref lid), & &TypeOperator(ref op)) => {
            if (occurs(lid, rhs)) {
                let (location, l, r) = type_error::cond.raise(());
                fail!("{} Error: Recursive unification between {}\nand\n{}", location, l, r);
            }
            let mut t = (*rhs).clone();
            if lhs.types.len() == 0 {
                replace(&mut env.constraints, &mut t, subs);
                subs.subs.insert(lid.clone(), t);
            }
            else {
                if lhs.types.len() != rhs.types.len() {
                let (location, l, r) = type_error::cond.raise(());
                    fail!("{} Error: Types do not have the same arity.\n{}\nand\n{}", location, l, r);
                }
                let mut x = Type::new_op(op.name.clone(), ~[]);
                replace(&mut env.constraints, &mut x, subs);
                subs.subs.insert(lid.clone(), x);
                for i in range(0, lhs.types.len()) {
                    unify_(env, subs, &mut lhs.types[i], &mut rhs.types[i]);
                    if i < lhs.types.len() - 1 {
                        replace(&mut env.constraints, &mut lhs.types[i+1], subs);
                        replace(&mut env.constraints, &mut rhs.types[i+1], subs);
                    }
                }
            }
            //Check that the type operator has an instance for all the constraints of the variable
            match env.constraints.find(lid) {
                Some(constraints) => {
                    for c in constraints.iter() {
                        if !env.has_instance(*c, rhs) {
                            if c.equiv(& &"Num") && (op.name.equiv(& &"Int") || op.name.equiv(& &"Double")) && rhs.types.len() == 0 {
                                continue;
                            }
                            else if c.equiv(& &"Fractional") && "Double" == op.name && rhs.types.len() == 0 {
                                continue;
                            }
                            else {
                                let (location, l, r) = type_error::cond.raise(());
                                fail!("{} Error: The instance {} {} was not found as required by {} when unifying {}\nand\n{}", location, *c, *op, *lid, l, r);
                            }
                        }
                    }
                }
                None => ()
            }
            true
        }
        _ => false
    };
    if !unified {
        return unify_(env, subs, rhs, lhs);
    }

}

///Creates a graph containing a vertex for each binding and edges for each 
fn build_graph(bindings: &Bindings) -> Graph<(uint, uint)> {
    let mut graph = Graph::new();
    let mut map = HashMap::new();
    bindings.each_binding(|bind, i| {
        let index = graph.new_vertex(i);
        map.insert(bind.name.clone(), index);
    });
    bindings.each_binding(|bind, _| {
        add_edges(&mut graph, &map, *map.get(&bind.name), &bind.expression);
    });
    graph
}

fn add_edges<T>(graph: &mut Graph<T>, map: &HashMap<~str, VertexIndex>, function_index: VertexIndex, expr: &TypedExpr) {
    match &expr.expr {
        &Identifier(ref n) => {
            match map.find_equiv(n) {
                Some(index) => graph.connect(function_index, *index),
                None => ()
            }
        }
        &Lambda(_, ref body) => {
            add_edges(graph, map, function_index, *body);
        }
        &Apply(ref f, ref a) => {
            add_edges(graph, map, function_index, *f);
            add_edges(graph, map, function_index, *a);
        }
        &Let(ref binds, ref body) => {
            add_edges(graph, map, function_index, *body);
            for bind in binds.iter() {
                add_edges(graph, map, function_index, &bind.expression);
            }
        }
        &Case(ref b, ref alts) => {
            add_edges(graph, map, function_index, *b);
            for alt in alts.iter() {
                add_edges(graph, map, function_index, &alt.expression);
            }
        }
        _ => ()
    }
}

fn each_type(typ: &Type, var_fn: |&TypeVariable|, op_fn: |&TypeOperator|) {
    each_type_(typ, &var_fn, &op_fn);
}
fn each_type_(typ: &Type, var_fn: &|&TypeVariable|, op_fn: &|&TypeOperator|) {
    match &typ.typ {
        &TypeVariable(ref var) => (*var_fn)(var),
        &TypeOperator(ref op) => (*op_fn)(op)
    }
    for t in typ.types.iter() {
        each_type_(t, var_fn, op_fn);
    }
}

pub fn function_type(func : &Type, arg : &Type) -> Type {
    Type::new_op(~"->", ~[func.clone(), arg.clone()])
}

#[cfg(test)]
pub fn identifier(i : ~str) -> TypedExpr {
    TypedExpr::new(Identifier(i))
}
#[cfg(test)]
pub fn lambda(arg : ~str, body : TypedExpr) -> TypedExpr {
    TypedExpr::new(Lambda(arg, ~body))
}
#[cfg(test)]
pub fn number(i : int) -> TypedExpr {
    TypedExpr::new(Number(i))
}
#[cfg(test)]
pub fn rational(i : f64) -> TypedExpr {
    TypedExpr::new(Rational(i))
}
#[cfg(test)]
pub fn apply(func : TypedExpr, arg : TypedExpr) -> TypedExpr {
    TypedExpr::new(Apply(~func, ~arg))
}
#[cfg(test)]
pub fn let_(bindings : ~[Binding], expr : TypedExpr) -> TypedExpr {
    TypedExpr::new(Let(bindings, ~expr))
}
#[cfg(test)]
pub fn case(expr : TypedExpr, alts: ~[Alternative]) -> TypedExpr {
    TypedExpr::new(Case(~expr, alts))
}

#[cfg(test)]
mod test {
use module::*;
use typecheck::*;

use parser::Parser;
use std::io::File;
use std::str::from_utf8;

#[test]
fn application() {
    let mut env = TypeEnvironment::new();
    let n = ~TypedExpr::new(Identifier(~"add"));
    let num = ~TypedExpr::new(Number(1));
    let mut expr = TypedExpr::new(Apply(n, num));
    let type_int = Type::new_op(~"Int", ~[]);
    let unary_func = function_type(&type_int, &type_int);
    let add_type = function_type(&type_int, &unary_func);
    env.namedTypes.insert(~"add", add_type);
    env.typecheck(&mut expr);

    let expr_type = expr.typ;
    assert!(expr_type == unary_func);
}

#[test]
fn typecheck_lambda() {
    let mut env = TypeEnvironment::new();
    let type_int = Type::new_op(~"Int",~[]);
    let unary_func = function_type(&type_int, &type_int);
    let add_type = function_type(&type_int, &unary_func);

    let mut expr = lambda(~"x", apply(apply(identifier(~"add"), identifier(~"x")), number(1)));
    env.namedTypes.insert(~"add", add_type);
    env.typecheck(&mut expr);

    assert_eq!(expr.typ, unary_func);
}

#[test]
fn typecheck_let() {
    let mut env = TypeEnvironment::new();
    let type_int = Type::new_op(~"Int", ~[]);
    let unary_func = function_type(&type_int, &type_int);
    let add_type = function_type(&type_int, &unary_func);

    //let test x = add x in test
    let unary_bind = lambda(~"x", apply(apply(identifier(~"add"), identifier(~"x")), number(1)));
    let mut expr = let_(~[Binding { arity: 1, name: ~"test", expression: unary_bind, typeDecl: Default::default() }], identifier(~"test"));
    env.namedTypes.insert(~"add", add_type);
    env.typecheck(&mut expr);

    assert_eq!(expr.typ, unary_func);
}

#[test]
fn typecheck_case() {
    let mut env = TypeEnvironment::new();
    let type_int = Type::new_op(~"Int", ~[]);
    let unary_func = function_type(&type_int, &type_int);
    let add_type = function_type(&type_int, &unary_func);

    let mut parser = Parser::new("case [] of { : x xs -> add x 2 ; [] -> 3}".chars());
    let mut expr = parser.expression_();
    env.namedTypes.insert(~"add", add_type);
    env.typecheck(&mut expr);

    assert_eq!(expr.typ, type_int);
    match &expr.expr {
        &Case(ref case_expr, _) => {
            assert_eq!(case_expr.typ, Type::new_op(~"[]", ~[Type::new_op(~"Int", ~[])]));
        }
        _ => fail!("typecheck_case")
    }
}

#[test]
fn typecheck_list() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new(
r"mult2 x = primIntMultiply x 2

main = case [mult2 123, 0] of
    : x xs -> x
    [] -> 10".chars());
    let mut module = parser.module();
    env.typecheck_module(&mut module);

    assert_eq!(module.bindings[1].expression.typ, Type::new_op(~"Int", ~[]));
}

#[test]
fn typecheck_string() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new("\"hello\"".chars());
    let mut expr = parser.expression_();
    env.typecheck(&mut expr);

    assert_eq!(expr.typ, Type::new_op(~"[]", ~[Type::new_op(~"Char", ~[])]));
}

#[test]
fn typecheck_tuple() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new("(primIntAdd 0 0, \"a\")".chars());
    let mut expr = parser.expression_();
    env.typecheck(&mut expr);

    let list = Type::new_op(~"[]", ~[Type::new_op(~"Char", ~[])]);
    assert_eq!(expr.typ, Type::new_op(~"(,)", ~[Type::new_op(~"Int", ~[]), list]));
}

#[test]
fn typecheck_module() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new(
r"data Bool = True | False
test x = True".chars());
    let mut module = parser.module();
    env.typecheck_module(&mut module);

    let typ = function_type(&Type::new_var(0), &Type::new_op(~"Bool", ~[]));
    let bind_type0 = module.bindings[0].expression.typ;
    assert_eq!(bind_type0, typ);
}


#[test]
fn typecheck_recursive_let() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new(
r"let
    a = primIntAdd 0 1
    test = primIntAdd 1 2 : test2
    test2 = 2 : test
    b = test
in b".chars());
    let mut expr = parser.expression_();
    env.typecheck(&mut expr);

    
    let int_type = Type::new_op(~"Int", ~[]);
    let list_type = Type::new_op(~"[]", ~[int_type.clone()]);
    match &expr.expr {
        &Let(ref binds, _) => {
            assert_eq!(binds.len(), 4);
            assert_eq!(binds[0].name, ~"a");
            assert_eq!(binds[0].expression.typ, int_type);
            assert_eq!(binds[1].name, ~"test");
            assert_eq!(binds[1].expression.typ, list_type);
        }
        _ => fail!("Error")
    }
}

#[test]
fn typecheck_constraints() {
    let mut parser = Parser::new(
r"class Test a where
    test :: a -> Int

instance Test Int where
    test x = 10

main = test 1".chars());

    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);

    let typ = &module.bindings[0].expression.typ;
    assert_eq!(typ, &Type::new_op(~"Int", ~[]));
}

//Test that calling a function with constraints will propagate the constraints to
//the type of the caller
#[test]
fn typecheck_constraints2() {
    let mut parser = Parser::new(
r"class Test a where
    test :: a -> Int

instance Test Int where
    test x = 10

main x y = primIntAdd (test x) (test y)".chars());

    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);

    let typ = &module.bindings[0].expression.typ;
    let int_type = Type::new_op(~"Int", ~[]);
    let test = function_type(&Type::new_var(-1),  &function_type(&Type::new_var(-2), &int_type));
    assert_eq!(typ, &test);
    let test_cons = ~[~"Test"];
    assert_eq!(env.constraints.find(typ.types[0].var()), Some(&test_cons));
    let second_fn = &typ.types[1];
    assert_eq!(env.constraints.find(second_fn.types[0].var()), Some(&test_cons));
}

#[test]
#[should_fail]
fn typecheck_constraints_no_instance() {
    let mut parser = Parser::new(
r"class Test a where
    test :: a -> Int

instance Test Int where
    test x = 10

main = test [1]".chars());

    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);
}

#[test]
fn typecheck_instance_super_class() {
    let mut parser = Parser::new(
r"data Bool = True | False

class Eq a where
    (==) :: a -> a -> Bool

instance Eq a => Eq [a] where
    (==) xs ys = case xs of
        : x2 xs2 -> case ys of
            : y2 ys2 -> (x2 == y2) && (xs2 == ys2)
            [] -> False
        [] -> case ys of
            : y2 ys2 -> False
            [] -> True

(&&) :: Bool -> Bool -> Bool
(&&) x y = case x of
    True -> y
    False -> False
".chars());

    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);

    let typ = &module.instances[0].bindings[0].expression.typ;
    let list_type = Type::new_op(~"[]", ~[Type::new_var(100)]);
    assert_eq!(*typ, function_type(&list_type, &function_type(&list_type, &Type::new_op(~"Bool", ~[]))));
    let var = typ.types[0].types[0].var();
    let eq = ~[~"Eq"];
    assert_eq!(env.constraints.find(var), Some(&eq));
}

#[test]
fn typecheck_num_double() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new(
r"test x = primDoubleAdd 0 x".chars());
    let mut module = parser.module();
    env.typecheck_module(&mut module);

    let typ = function_type(&Type::new_op(~"Double", ~[]), &Type::new_op(~"Double", ~[]));
    let bind_type0 = module.bindings[0].expression.typ;
    assert_eq!(bind_type0, typ);
}

#[test]
fn typecheck_functor() {
    let mut env = TypeEnvironment::new();

    let mut parser = Parser::new(
r"data Maybe a = Just a | Nothing

class Functor f where
    fmap :: (a -> b) -> f a -> f b

instance Functor Maybe where
    fmap f x = case x of
        Just y -> Just (f y)
        Nothing -> Nothing

add2 x = primIntAdd x 2
main = fmap add2 (Just 3)".chars());
    let mut module = parser.module();
    env.typecheck_module(&mut module);

    let main = &module.bindings[1];
    assert_eq!(main.expression.typ, Type::new_op(~"Maybe", ~[Type::new_op(~"Int", ~[])]));
}

#[test]
fn typecheck_prelude() {
    let path = &Path::new("Prelude.hs");
    let s  = File::open(path).read_to_end();
    let contents : &str = from_utf8(s);
    let mut parser = Parser::new(contents.chars());
    let mut module = parser.module();
    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);

    let id = module.bindings.iter().find(|bind| bind.name == ~"id");
    assert!(id != None);
    let id_bind = id.unwrap();
    assert_eq!(id_bind.expression.typ, function_type(&Type::new_var(0), &Type::new_var(0)));
}

#[test]
fn typecheck_import() {
   
    let prelude = {
        let path = &Path::new("Prelude.hs");
        let s  = File::open(path).read_to_end();
        let contents : &str = from_utf8(s);
        let mut parser = Parser::new(contents.chars()); 
        let mut module = parser.module();
        let mut env = TypeEnvironment::new();
        env.typecheck_module(&mut module);
        module
    };

    let mut parser = Parser::new(
r"
test1 = map not [True, False]
test2 = id (primIntAdd 2 0)".chars());
    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.add_types(&prelude as &Types);
    env.typecheck_module(&mut module);

    assert_eq!(module.bindings[0].name, ~"test1");
    assert_eq!(module.bindings[0].expression.typ, Type::new_op(~"[]", ~[Type::new_op(~"Bool", ~[])]));
    assert_eq!(module.bindings[1].name, ~"test2");
    assert_eq!(module.bindings[1].expression.typ, Type::new_op(~"Int", ~[]));
}

#[test]
fn type_declaration() {
    
    let mut parser = Parser::new(
r"
class Test a where
    test :: a -> Int

instance Test Int where
    test x = x

test :: Test a => a -> Int -> Int
test x y = primIntAdd (test x) y".chars());
    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);

    assert_eq!(module.bindings[0].typeDecl.typ, module.typeDeclarations[0].typ);
}

#[test]
#[should_fail]
fn type_declaration_error() {
    
    let mut parser = Parser::new(
r"
test :: [Int] -> Int -> Int
test x y = primIntAdd x y".chars());
    let mut module = parser.module();

    let mut env = TypeEnvironment::new();
    env.typecheck_module(&mut module);
}

}

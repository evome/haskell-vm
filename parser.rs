use std::util::{swap};
use std::hashmap::HashMap;
use lexer::{Lexer, Token, TokenEnum,
    EOF, NAME, OPERATOR, NUMBER, FLOAT, STRING, CHAR, LPARENS, RPARENS, LBRACKET, RBRACKET, LBRACE, RBRACE, COMMA, EQUALSSIGN, SEMICOLON, MODULE, CLASS, INSTANCE, WHERE, LET, IN, CASE, OF, ARROW, TYPEDECL, DATA
};
use module::*;
use typecheck::function_type;

pub struct Parser<Iter> {
    lexer : Lexer<Iter>,
}

impl <Iter : Iterator<char>> Parser<Iter> {

pub fn new(iterator : Iter) -> Parser<Iter> {
    Parser { lexer : Lexer::new(iterator) }
}

fn requireNext<'a>(&'a mut self, expected : TokenEnum) -> &'a Token {
	let tok = self.lexer.next_().token;
	if (tok != expected) {
		fail!(ParseError(&self.lexer, expected));
    }
	return self.lexer.current();
}

pub fn module(&mut self) -> Module {
	let lBracketOrModule = self.lexer.module_next().token;//tokenizeModule??
	let modulename = match lBracketOrModule {
        MODULE => {
            let modulename = self.requireNext(NAME).value.clone();
            self.requireNext(WHERE);
            self.requireNext(LBRACE);
            modulename
	    }
        LBRACE => {
		    //No module declaration was found so default to Main
		    ~"Main"
	    }
        _ => fail!(ParseError(&self.lexer, LBRACE))
    };

    let mut classes = ~[];
    let mut bindings = ~[];
    let mut instances = ~[];
    let mut typeDeclarations = ~[];
    let mut dataDefinitions = ~[];
	loop {
		//Do a lookahead to see what the next top level binding is
		let token = self.lexer.next(toplevelError).token;
		if (token == NAME || token == LPARENS)
		{
            let mut equalOrType = self.lexer.next(bindingError).token;
            {
			    let mut numberOfLookaheads = 2;
                while (equalOrType != TYPEDECL
                    && equalOrType != EQUALSSIGN)
                {
                    equalOrType = self.lexer.next(bindingError).token;
                    numberOfLookaheads += 1;
                }
                for _ in range(0, numberOfLookaheads)
                {
                    self.lexer.backtrack();
                }
            }

			if (equalOrType == TYPEDECL)
			{
				let bind = self.typeDeclaration();
				typeDeclarations.push(bind);
			}
			else
			{
				let bind = self.binding();
                debug!("Parsed binding {}", bind.name);
				bindings.push(bind);
			}
		}
		else if (token == CLASS)
		{
			self.lexer.backtrack();
			classes.push(self.class());
		}
		else if (token == INSTANCE)
		{
			self.lexer.backtrack();
			instances.push(self.instance());
		}
		else if (token == DATA)
		{
			self.lexer.backtrack();
			dataDefinitions.push(self.dataDefinition());
		}
		else
		{
			break;
		}
		let semicolon = self.lexer.next(toplevelNewBindError);
        debug!("More bindings? {:?}", semicolon.token);
	    if (semicolon.token != SEMICOLON) {
            break;
        }
    }

	let rBracket = self.lexer.current().token;
	if (rBracket != RBRACE)
	{
		fail!(ParseError(&self.lexer, RBRACE));
	}

	let eof = self.lexer.next_();
	if (eof.token != EOF)
	{
		fail!("Unexpected token after end of module, {:?}", eof.token);
	}

	for decl in typeDeclarations.mut_iter()
	{
		for bind in bindings.mut_iter()
		{
			if (decl.name == bind.name)
			{
				bind.typeDecl = (*decl).clone();
			}
		}
	}
    Module {
        name : modulename,
        bindings : bindings,
        typeDeclarations : typeDeclarations,
        classes : classes,
        instances : instances,
        dataDefinitions : dataDefinitions }
}

fn class(&mut self) -> Class {
	self.requireNext(CLASS);

	let classname = self.requireNext(NAME).value.clone();
	let typeVariableName = self.requireNext(NAME).value.clone();
    let typeVariable = 1000000;

	self.requireNext(WHERE);
	self.requireNext(LBRACE);
	let mut typeVariableMapping = HashMap::new();
	typeVariableMapping.insert(typeVariableName, typeVariable);
	let declarations = self.sepBy1(|this| this.typeDeclaration_(&mut typeVariableMapping), SEMICOLON);
	
	self.lexer.backtrack();
	self.requireNext(RBRACE);

	Class { name : classname, variable: TypeVariable { id: typeVariable }, declarations : declarations }
}

fn instance(&mut self) -> Instance {
	self.requireNext(INSTANCE);

    let mut mapping = HashMap::new();
    let (constraints, instance_type) = self.constrained_type(&mut mapping);
    match instance_type {
        Type { typ: TypeOperator(TypeOperator { name: classname}), types: types } => {
            self.requireNext(WHERE);
            self.requireNext(LBRACE);

            let mut bindings = self.sepBy1(|this| this.binding(), SEMICOLON);
            for bind in bindings.mut_iter()
            {
                bind.name = encodeBindingIdentifier(types[0].op().name, bind.name);
            }

            self.lexer.backtrack();
            self.requireNext(RBRACE);
            Instance { typ : types[0], classname : classname, bindings : bindings, constraints: constraints }
        }
        _ => fail!("TypeVariable in instance")
    }
}

pub fn expression_(&mut self) -> TypedExpr {
    match self.expression() {
        Some(expr) => expr,
        None => fail!("Failed to parse expression at {:?}", self.lexer.current().location)
    }
}

pub fn expression(&mut self) -> Option<TypedExpr> {
	let app = self.application();
	self.parseOperatorExpression(app, 0)
}


fn parseList(&mut self) -> TypedExpr {
	let mut expressions = ~[];
	loop {
		match self.expression() {
            Some(expr) => expressions.push(expr),
            None => break
        }
		let comma = self.lexer.next_().token;
        if (comma != COMMA) {
            self.lexer.backtrack();
            break;
        }
	}
    self.requireNext(RBRACKET);

	if (expressions.len() == 0)
	{
		return TypedExpr::new(Identifier(~"[]"));
	}

	let mut application;
	{
		let mut arguments = ~[TypedExpr::new(Number(0)), TypedExpr::new(Number(0))];//Must be 2 in length
		swap(&mut arguments[0], &mut expressions[expressions.len() - 1]);
		expressions.pop();
		arguments[1] = TypedExpr::new(Identifier(~"[]"));

		application = makeApplication(TypedExpr::new(Identifier(~":")), arguments);
	}
	while (expressions.len() > 0)
	{
		let mut arguments = ~[TypedExpr::new(Number(0)), TypedExpr::new(Number(0))];//Must be 2 in length
		swap(&mut arguments[0], &mut expressions[expressions.len() - 1]);
		expressions.pop();
		arguments[1] = application;

		application = makeApplication(TypedExpr::new(Identifier(~":")), arguments);
	}
    application
}

fn subExpression(&mut self, parseError : |&Token| -> bool) -> Option<TypedExpr> {
	let token = self.lexer.next(parseError).token;
    debug!("Begin SubExpr {:?}", self.lexer.current());
	match token {
	    LPARENS =>
		{
			let expressions = self.sepBy1(|this| this.expression_(), COMMA);

			let maybeParens = self.lexer.current();

			if (maybeParens.token != RPARENS)
			{
				fail!(ParseError(&self.lexer, RPARENS));
			}
			if (expressions.len() == 1)
			{
				Some(expressions[0])
			}
			else
			{
				Some(newTuple(expressions))
			}
		}
	    LBRACKET => Some(self.parseList()),
	    LET =>
		{
			self.requireNext(LBRACE);

			let binds = self.sepBy1(|this| this.binding(), SEMICOLON);

			let rBracket = self.lexer.current().token;
			if (rBracket != RBRACE)
			{
				fail!(ParseError(&self.lexer, RBRACE));
			}
			let inToken = self.lexer.next(letExpressionEndError).token;
			if (inToken != IN) {
				fail!(ParseError(&self.lexer, IN));
            }
			match self.expression() {
                Some(e) => {
                    Some(TypedExpr::new(Let(binds, ~e)))
                }
                None => None
            }
		}
	    CASE =>
		{
            let location = self.lexer.current().location;
			let expr = self.expression();

			self.requireNext(OF);
			self.requireNext(LBRACE);

			let alts = self.sepBy1(|this| this.alternative(), SEMICOLON);
			let rBrace = self.lexer.current();
			if (rBrace.token != RBRACE)
			{
				fail!(ParseError(&self.lexer, RBRACE));
			}
			match expr {
                Some(e) => Some(TypedExpr::with_location(Case(~e, alts), location)),
                None => None
            }
		}
        NAME => {
            let token = self.lexer.current();
            Some(TypedExpr::with_location(Identifier(token.value.clone()), token.location))
        }
        NUMBER => {
            let token = self.lexer.current();
            Some(TypedExpr::with_location(Number(from_str(token.value).unwrap()), token.location))
        }
	    FLOAT => {
            let token = self.lexer.current();
            Some(TypedExpr::with_location(Rational(from_str(token.value).unwrap()), token.location))
        }
        STRING => {
            let token = self.lexer.current();
            Some(TypedExpr::with_location(String(token.value.clone()), token.location))
        }
        CHAR => {
            let token = self.lexer.current();
            Some(TypedExpr::with_location(Char(token.value.char_at(0)), token.location))
        }
	    _ => {
            self.lexer.backtrack();
            None
        }
    }
}

fn alternative(&mut self) -> Alternative {
	let pat = self.located_pattern();

	self.requireNext(ARROW);

	Alternative { pattern : pat, expression : self.expression_() }
}

fn parseOperatorExpression(&mut self, inL : Option<TypedExpr>, minPrecedence : int) -> Option<TypedExpr> {
	let mut lhs = inL;
    self.lexer.next_();
    debug!("Parse operator exression, {:?}", self.lexer.current());
	while (self.lexer.valid() && self.lexer.current().token == OPERATOR
		&& precedence(self.lexer.current().value) >= minPrecedence)
	{
		let op = (*self.lexer.current()).clone();
		let mut rhs = self.application();
		self.lexer.next_();
        debug!("Parsing operator? {:?}", self.lexer.current());
		while (self.lexer.valid() && self.lexer.current().token == OPERATOR
			&& precedence(self.lexer.current().value) >= precedence(op.value))
		{
			let lookaheadPrecedence = precedence(self.lexer.current().value);
			self.lexer.backtrack();
			rhs = self.parseOperatorExpression(rhs, lookaheadPrecedence);
            self.lexer.next_();
		}
		let mut name = TypedExpr::with_location(Identifier(op.value.clone()), op.location);
		let loc = match &lhs {
            &Some(ref l) => l.location,
            &None => op.location
        };
        lhs = match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => {
                let args = ~[lhs, rhs];
                Some(makeApplication(name, args))
            }
            (Some(lhs), None) => {
                Some(TypedExpr::with_location(Apply(~name, ~lhs), loc))
            }
            (None, Some(rhs)) => {
                if (op.value == ~"-")
                {
                    match name.expr {
                        Identifier(ref mut n) => *n = ~"negate",
                        _ => fail!("WTF")
                    }
                    let args = ~[rhs];
                    Some(makeApplication(name, args))
                }
                else
                {
                    let args = ~[TypedExpr::with_location(Identifier(~"#"), loc), rhs];
                    let mut apply = makeApplication(name, args);
                    apply.location = loc;
                    let params = ~[~"#"];
                    Some(makeLambda(params, apply))
                }
            }
            (None, None) => return None
        };
	}
	self.lexer.backtrack();
	lhs
}

fn application(&mut self) -> Option<TypedExpr> {
    let e = self.subExpression(|_| false);
	match e {
        Some(mut lhs) => {
            let mut expressions = ~[];
            loop {
                let expr = self.subExpression(applicationError);
                match expr {
                    Some(e) => expressions.push(e),
                    None => break
                }
            }
            if (expressions.len() > 0)
            {
                let loc = lhs.location;
                lhs = makeApplication(lhs, expressions);//, loc);
                lhs.location = loc;
            }
            Some(lhs)
        }
        None => None
    }
}

fn constructor(&mut self, dataDef : &DataDefinition) -> Constructor {
	let name = self.requireNext(NAME).value.clone();
	let mut arity = 0;
    let mut mapping = dataDef.parameters.clone();
	let typ = self.constructorType(&mut arity, dataDef, &mut mapping);
	self.lexer.backtrack();
	Constructor { name : name, typ : typ, tag : 0, arity : arity }
}

fn binding(&mut self) -> Binding {
    debug!("Begin binding");
	//name1 = expr
	//or
	//name2 x y = expr
	let nameToken = self.lexer.next(errorIfNotNameOrLParens).token;
	let mut name = self.lexer.current().value.clone();
	if (nameToken == LPARENS)
	{
		//Parse a name within parentheses
		let functionName = self.lexer.next(errorIfNotNameOrOperator).token;
		if (functionName != NAME && functionName != OPERATOR)
		{
			fail!("Expected NAME or OPERATOR on left side of binding {:?}", self.lexer.current().token);
		}
		name = self.lexer.current().value.clone();

		let rParens = self.lexer.next(errorIfNotRParens).token;
		if (rParens != RPARENS)
		{
			fail!(ParseError(&self.lexer, RPARENS));
		}
	}
	else if (nameToken != NAME)
	{
		fail!(ParseError(&self.lexer, NAME));
	}

	//Parse the arguments for the binding
	let mut arguments = ~[];
	while (true)
	{
		let token = self.lexer.next(errorIfNotNameOrEqual);
		if (token.token == NAME)
		{
			arguments.push(token.value.clone());
		}
		else
		{
			break;
		}
	}
	if (self.lexer.current().token != EQUALSSIGN)
	{
		fail!(ParseError(&self.lexer, EQUALSSIGN));
	}
	if (arguments.len() > 0)
    {
        let arity = arguments.len();
		let lambda = makeLambda(arguments, self.expression_());
		Binding { name : name, typeDecl : TypeDeclaration { context : ~[], typ : Type::new_var(-1), name : ~"" }, expression : lambda, arity : arity }
	}
	else
	{
		Binding { name : name, typeDecl : TypeDeclaration { context : ~[], typ : Type::new_var(-1), name : ~"" }, expression : self.expression_(), arity : 0 }
	}
}


fn patternParameter(&mut self) -> ~[Pattern] {
	let mut parameters = ~[];
	loop {
		let token = self.lexer.next_().token;
		match token
		{
            NAME => parameters.push(IdentifierPattern(self.lexer.current().value.clone())),
            NUMBER => parameters.push(NumberPattern(from_str(self.lexer.current().value.clone()).unwrap())),
		    LPARENS =>
			{
				let pat = self.pattern();
				let maybeComma = self.lexer.next_().token;
				if (maybeComma == COMMA)
				{
					let mut tupleArgs = self.sepBy1(|this| this.pattern(), COMMA);

					let rParens = self.lexer.current();
					if (rParens.token != RPARENS)
					{
						fail!(ParseError(&self.lexer, RPARENS));
					}
					tupleArgs.unshift(pat);
					parameters.push(ConstructorPattern(tuple_name(tupleArgs.len()), tupleArgs));
				}
				else
				{
                    //TODO?
				}
			}
            LBRACKET => {
                if (self.lexer.next_().token != RBRACKET)
                {
                    fail!(ParseError(&self.lexer, RBRACKET));
                }
                parameters.push(ConstructorPattern(~"[]", ~[]));
            }
		    _ => { break; }
		}
	}
	self.lexer.backtrack();
	return parameters;
}

fn located_pattern(&mut self) -> Located<Pattern> {
    let location = self.lexer.next_().location;
    self.lexer.backtrack();
    Located { location: location, node: self.pattern() }
}

fn pattern(&mut self) -> Pattern {
	let nameToken = self.lexer.next_().token;
    let name = self.lexer.current().value.clone();
	match nameToken {
	    LBRACKET =>
		{
			if (self.lexer.next_().token != RBRACKET)
			{
				fail!(ParseError(&self.lexer, RBRACKET));
			}
			ConstructorPattern(~"[]", ~[])
		}
	    NAME | OPERATOR =>
		{
			let patterns = self.patternParameter();
			if (name.char_at(0).is_uppercase() || name == ~":")
			{
				ConstructorPattern(name, patterns)
			}
			else
			{
				assert!(patterns.len() == 0);
				IdentifierPattern(name)
			}
		}
	    NUMBER => NumberPattern(from_str(name).unwrap()),
	    LPARENS =>
		{
			let tupleArgs = self.sepBy1(|this| this.pattern(), COMMA);
			let rParens = self.lexer.current().token;
			if (rParens != RPARENS) {
				fail!(ParseError(&self.lexer, RPARENS));
			}
			ConstructorPattern(tuple_name(tupleArgs.len()), tupleArgs)
		}
	    _ => { fail!("Error parsing pattern") }
	}
}

fn typeDeclaration(&mut self) -> TypeDeclaration {
	let mut typeVariableMapping = HashMap::new();
	self.typeDeclaration_(&mut typeVariableMapping)
}

fn typeDeclaration_(&mut self, typeVariableMapping : &mut HashMap<~str, int>) -> TypeDeclaration {
    let mut name;
	{
        let nameToken = self.lexer.next(errorIfNotNameOrLParens).token;
        name = self.lexer.current().value.clone();
        if (nameToken == LPARENS) {
            //Parse a name within parentheses
            let functionName = self.lexer.next(errorIfNotNameOrOperator).token;
            if (functionName != NAME && functionName != OPERATOR)
            {
                fail!("Expected NAME or OPERATOR on left side of binding {:?}", functionName);
            }
            name = self.lexer.current().value.clone();
            let rParens = self.lexer.next(errorIfNotRParens).token;
            if (rParens != RPARENS)
            {
                fail!(ParseError(&self.lexer, RPARENS));
            }
        }
        else if (nameToken != NAME) {
            fail!(ParseError(&self.lexer, NAME));
        }
    }
	let decl = self.lexer.next_().token;
	if (decl != TYPEDECL) {
		fail!(ParseError(&self.lexer, TYPEDECL));
	}
    let (context, typ) = self.constrained_type(typeVariableMapping);
	TypeDeclaration { name : name, typ : typ, context : context }
}

fn constrained_type(&mut self, typeVariableMapping : &mut HashMap<~str, int>) -> (~[Constraint], Type) {
    let mut variableIndex = 0;
	let typeOrContext = self.parse_type_(&mut variableIndex, typeVariableMapping);
    {
        let maybeContextArrow = self.lexer.next_().token;
        if (maybeContextArrow == OPERATOR && self.lexer.current().value == ~"=>") {
            let t = self.parse_type_(&mut variableIndex, typeVariableMapping);
            return (createTypeConstraints(typeOrContext), t);
        }
    }
	self.lexer.backtrack();
	(~[], typeOrContext)
}

fn constructorType(&mut self, arity : &mut int, dataDef: &DataDefinition, mapping : &mut HashMap<~str, int>) -> Type
{
	let token = self.lexer.next(constructorError).token;
	if (token == NAME) {
		*arity += 1;
		let arg = if (self.lexer.current().value.char_at(0).is_lowercase())
		{
			match mapping.find(&self.lexer.current().value) {
                Some(existingVariable) => Type::new_var(*existingVariable),
                None => fail!("Undefined type parameter {:?}", self.lexer.current().value)
            }
		}
		else {
			Type::new_op(self.lexer.current().value.clone(), ~[])
        };
        function_type(&arg, &self.constructorType(arity, dataDef, mapping))
	}
	else if token == LPARENS {
        *arity += 1;
        let mut var = 100000;
        let arg = self.parse_type_(&mut var, mapping);
        self.requireNext(RPARENS);
        function_type(&arg, &self.constructorType(arity, dataDef, mapping))
    }
    else {
		dataDef.typ.clone()
	}
}


fn dataDefinition(&mut self) -> DataDefinition {
	self.requireNext(DATA);
	let dataName = self.requireNext(NAME).value.clone();

	let mut definition = DataDefinition {
        constructors : ~[],
        typ : Type::new_var(0),
        parameters : HashMap::new()
    };
    definition.typ.typ = TypeOperator(TypeOperator { name: dataName });
	while (self.lexer.next_().token == NAME)
	{
        //TODO use new variables isntead of only  -1
		definition.typ.types.push(Type::new_var(-1));
		definition.parameters.insert(self.lexer.current().value.clone(), -1);
	}

	let equalToken = self.lexer.current().token;
	if (equalToken != EQUALSSIGN)
	{
		fail!(ParseError(&self.lexer, EQUALSSIGN));
	}
	definition.constructors = self.sepBy1_func(|this| this.constructor(&definition),
		|t : &Token| t.token == OPERATOR && t.value == ~"|");
	for ii in range(0, definition.constructors.len())
	{
		definition.constructors[ii].tag = ii as int;
	}
	self.lexer.backtrack();
	definition
}

fn sub_type(&mut self, variableIndex: &mut int, typeVariableMapping: &mut HashMap<~str, int>) -> Option<Type> {
	let token = (*self.lexer.next_()).clone();
	match token.token {
	    LBRACKET =>
		{
            self.lexer.backtrack();
            Some(self.parse_type_(variableIndex, typeVariableMapping))
		}
	    LPARENS =>
		{
            self.lexer.backtrack();
			Some(self.parse_type_(variableIndex, typeVariableMapping))
		}
	    NAME =>
		{
			if (token.value.char_at(0).is_uppercase()) {
				Some(Type::new_op(token.value, ~[]))
			}
			else {
                let t = typeVariableMapping.find_or_insert(token.value, *variableIndex);
                *variableIndex += 1;
				Some(Type::new_var(*t))
			}
		}
        _ => { self.lexer.backtrack(); None }
	}
}

fn parse_type_(&mut self, variableIndex: &mut int, typeVariableMapping : &mut HashMap<~str, int>) -> Type {
	let token = (*self.lexer.next_()).clone();
	match token.token {
	    LBRACKET =>
		{
            if self.lexer.next_().token == RBRACKET {
                let listType = Type::new_op(~"[]", ~[]);
                self.parse_return_type(listType, variableIndex, typeVariableMapping)
            }
            else {
                self.lexer.backtrack();
                let t = self.parse_type_(variableIndex, typeVariableMapping);
                self.requireNext(RBRACKET);
                let listType = Type::new_op(~"[]", ~[t]);
                
                self.parse_return_type(listType, variableIndex, typeVariableMapping)
            }
		}
	    LPARENS =>
		{
			let t = self.parse_type_(variableIndex, typeVariableMapping);
			let maybeComma = self.lexer.next_().token;
			if (maybeComma == COMMA)
			{
				let mut tupleArgs = self.sepBy1(|this| this.parse_type_(variableIndex, typeVariableMapping), COMMA);
				tupleArgs.unshift(t);
                self.lexer.backtrack();
                self.requireNext(RPARENS);

                self.parse_return_type(tupleType(tupleArgs), variableIndex, typeVariableMapping)
			}
			else if (maybeComma == RPARENS)
			{
                self.parse_return_type(t, variableIndex, typeVariableMapping)
			}
            else {
                fail!(ParseError2(&self.lexer, &[COMMA, RPARENS]))
            }
		}
	    NAME =>
		{
			let mut typeArguments = ~[];
            loop {
                match self.sub_type(variableIndex, typeVariableMapping) {
                    Some(typ) => typeArguments.push(typ),
                    None => break
                }
            }

			let thisType = if (token.value.char_at(0).is_uppercase()) {
				Type::new_op(token.value, typeArguments)
			}
			else {
                let t = typeVariableMapping.find_or_insert(token.value, *variableIndex);
                *variableIndex += 1;
				let mut result = Type::new_var(*t);
                result.types = typeArguments;
                result
			};
			self.parse_return_type(thisType, variableIndex, typeVariableMapping)
		}
	    _ => fail!("Unexpected token when parsing type {:?}", self.lexer.current())
	}
}

fn parse_return_type(&mut self, typ : Type, variableIndex: &mut int, typeVariableMapping : &mut HashMap<~str, int>) -> Type {

    let arrow = self.lexer.next_().token;
    if (arrow == ARROW) {
        return function_type(&typ, &self.parse_type_(variableIndex, typeVariableMapping));
    }
    else {
        self.lexer.backtrack();
        return typ
    }
}

fn sepBy1<T>(&mut self, f : |&mut Parser<Iter>| -> T, sep : TokenEnum) -> ~[T] {
    self.sepBy1_func(f, |tok| tok.token == sep)
}

fn sepBy1_func<T>(&mut self, f : |&mut Parser<Iter>| -> T, sep : |&Token| -> bool) -> ~[T] {
    let mut result = ~[];
    loop {
        result.push(f(self));
        if (!sep(self.lexer.next_())) {
            break;
        }
    }
    result
}
}//end impl Parser

fn precedence(s : &str) -> int {
    match s {
        "+" => 1,
        "-" => 1,
        "*" => 3,
        "/" => 3,
        "%" => 3,
        "==" => 1,
        "/=" => 1,
        "<" => 1,
        ">" => 1,
        "<=" => 1,
        ">=" => 1,
        _ => 9
    }
}


fn toplevelError(t : &Token) -> bool
{
	return t.token != NAME
		&& t.token != RBRACKET
		&& t.token != SEMICOLON
		&& t.token != DATA
		&& t.token != LPARENS
		&& t.token != CLASS
		&& t.token != INSTANCE;
}

fn toplevelNewBindError(t : &Token) -> bool
{
	return t.token != RBRACKET
		&& t.token != SEMICOLON;
}

fn bindingError(t : &Token) -> bool
{
	return t.token != EQUALSSIGN
		&& t.token != NAME
		&& t.token != TYPEDECL
		&& t.token != OPERATOR
		&& t.token != RPARENS;
}

fn constructorError(tok : &Token) -> bool
{
	return tok.token != NAME
		&& tok.token != OPERATOR
		&& tok.token != LPARENS;
}

fn tuple_name(size : uint) -> ~str
{
	let mut name = ~"(";
    for _ in range(1, size) {
        name.push_char(',');
    }
	name.push_char(')');
	name
}

fn makeApplication(f : TypedExpr, args : ~[TypedExpr]) -> TypedExpr {
	assert!(args.len() >= 1);
    let mut func = f;
	for a in args.move_iter() {
        let loc = func.location.clone();
		func = TypedExpr::with_location(Apply(~func, ~a), loc);
	}
    func
}
fn makeLambda(a : ~[~str], body : TypedExpr) -> TypedExpr {
    let mut args = a;
	assert!(args.len() >= 1);
	let mut body = body;
    let mut ii = args.len() as int - 1;
	while ii >= 0 {
        let loc = body.location.clone();
		body = TypedExpr::with_location(Lambda(args.pop(), ~body), loc);
        ii -= 1;
	}
    body
}

//Create a tuple with the constructor name inferred from the number of arguments passed in
fn newTuple(arguments : ~[TypedExpr]) -> TypedExpr {
	let name = TypedExpr::new(Identifier(tuple_name(arguments.len())));
	makeApplication(name, arguments)
}

fn letExpressionEndError(t : &Token) -> bool {
	t.token != IN
}

fn applicationError(t :&Token) -> bool
{
	return t.token != LPARENS
		&& t.token != RPARENS
		&& t.token != LBRACKET
		&& t.token != RBRACKET
		&& t.token != LET
		&& t.token != OF
		&& t.token != NAME
		&& t.token != NUMBER
		&& t.token != FLOAT
		&& t.token != OPERATOR
		&& t.token != SEMICOLON
		&& t.token != COMMA;
}


fn errorIfNotNameOrLParens(tok : &Token) -> bool {
    tok.token != NAME && tok.token != LPARENS
}
fn errorIfNotNameOrOperator(tok : &Token) -> bool {
	tok.token != NAME && tok.token != OPERATOR
}

fn errorIfNotNameOrEqual(tok : &Token) -> bool {
	tok.token != NAME && tok.token != EQUALSSIGN
}
fn errorIfNotRParens(tok : &Token) -> bool {
	tok.token != RPARENS
}

fn createTypeConstraints(context : Type) -> ~[Constraint] {
	let mut mapping = ~[];

	if (context.op().name.char_at(0) == '(') {
		for t in context.types.move_iter() {
            match t {
                Type { typ: TypeOperator(op), types: types } => {
		            mapping.push(Constraint { class: op.name, variables: ~[types[0].var().clone()] });
                }
                _ => fail!("Expected TypeOperator when creating constraints")
            }
		}
	}
	else {
        match context {
            Type { typ: TypeOperator(op), types: types } => {
		        mapping.push(Constraint { class: op.name, variables: ~[types[0].var().clone()] });
            }
            _ => fail!("Expected TypeOperator when creating constraints")
        }
	}
	mapping
}

fn tupleType(types : ~[Type]) -> Type {
	Type::new_op(tuple_name(types.len()), types)
}

fn ParseError2<Iter : Iterator<char>>(lexer : &Lexer<Iter>, expected : &[TokenEnum]) -> ~str {
    format!("Expected {:?} but found {:?}\\{{:?}\\}, at {}", expected, lexer.current().token, lexer.current().value, lexer.current().location)
    
}
fn ParseError<Iter : Iterator<char>>(lexer : &Lexer<Iter>, expected : TokenEnum) -> ~str {
    format!("Expected {:?} but found {:?}\\{{:?}\\}, at {}", expected, lexer.current().token, lexer.current().value, lexer.current().location)
}
fn encodeBindingIdentifier(instancename : &str, bindingname : &str) -> ~str {
    "#" + instancename.clone() + bindingname.clone()
}

#[cfg(test)]
mod tests {

use parser::*;
use module::*;
use typecheck::{function_type, identifier, apply, number, rational, lambda, let_, case};
use std::io::File;
use std::str::from_utf8;


#[test]
fn simple()
{
    let mut parser = Parser::new("2 + 3".chars());
    let expr = parser.expression_();
    assert_eq!(expr, apply(apply(identifier(~"+"), number(2)), number(3)));
}
#[test]
fn binding()
{
    let mut parser = Parser::new("test x = x + 3".chars());
    let bind = parser.binding();
    assert_eq!(bind.expression, lambda(~"x", apply(apply(identifier(~"+"), identifier(~"x")), number(3))));
    assert_eq!(bind.name, ~"test");
}

#[test]
fn double()
{
    let mut parser = Parser::new("test = 3.14".chars());
    let bind = parser.binding();
    assert_eq!(bind.expression, rational(3.14));
    assert_eq!(bind.name, ~"test");
}

#[test]
fn parse_let() {
    let mut parser = Parser::new(
r"
let
    test = add 3 2
in test - 2".chars());
    let expr = parser.expression_();
    let bind = Binding { arity: 0, name: ~"test", typeDecl:Default::default(),
        expression: apply(apply(identifier(~"add"), number(3)), number(2)) };
    assert_eq!(expr, let_(~[bind], apply(apply(identifier(~"-"), identifier(~"test")), number(2))));
}

#[test]
fn parse_case() {
    let mut parser = Parser::new(
r"case [] of
    : x xs -> x
    [] -> 2
".chars());
    let expression = parser.expression_();
    let alt = Alternative {
        pattern: Located {
            location: Location::eof(),
            node: ConstructorPattern(~":", ~[IdentifierPattern(~"x"), IdentifierPattern(~"xs")])
        },
        expression: identifier(~"x") };
    let alt2 = Alternative {
        pattern: Located { location: Location::eof(), node: ConstructorPattern(~"[]", ~[]) },
        expression: number(2) };
    assert_eq!(expression, case(identifier(~"[]"), ~[alt, alt2]));
}

#[test]
fn parse_type() {
    let mut parser = Parser::new(
r"(.) :: (b -> c) -> (a -> b) -> (a -> c)".chars());
    let typeDecl = parser.typeDeclaration();
    let a = &Type::new_var(0);
    let b = &Type::new_var(1);
    let c = &Type::new_var(2);
    let f = function_type(&function_type(b, c), &function_type(&function_type(a, b), &function_type(a, c)));

    assert_eq!(typeDecl.name, ~".");
    assert_eq!(typeDecl.typ, f);
}
#[test]
fn parse_data() {
    let mut parser = Parser::new(
r"data Bool = True | False".chars());
    let data = parser.dataDefinition();

    let Bool = Type::new_op(~"Bool", ~[]);
    let True = Constructor { name: ~"True", tag:0, arity:0, typ: Bool.clone() };
    let False = Constructor { name: ~"False", tag:1, arity:0, typ: Bool.clone() };
    assert_eq!(data.typ, Bool);
    assert_eq!(data.constructors[0], True);
    assert_eq!(data.constructors[1], False);
}

#[test]
fn parse_data_2() {
    let mut parser = Parser::new(
r"data List a = Cons a (List a) | Nil".chars());
    let data = parser.dataDefinition();

    let List = Type::new_op(~"List", ~[Type::new_var(0)]);
    let Cons = Constructor { name: ~"Cons", tag:0, arity:2, typ: function_type(&Type::new_var(0), &function_type(&List, &List))};
    let Nil = Constructor { name: ~"Nil", tag:1, arity:0, typ: List.clone() };
    assert_eq!(data.typ, List);
    assert_eq!(data.constructors[0], Cons);
    assert_eq!(data.constructors[1], Nil);
}

#[test]
fn parse_tuple() {
    let mut parser = Parser::new(
r"(1, x)".chars());
    let expr = parser.expression_();

    assert_eq!(expr, apply(apply(identifier(~"(,)"), number(1)), identifier(~"x")));
}

#[test]
fn test_operators() {
    let mut parser = Parser::new("1 : 2 : []".chars());
    let expr = parser.expression_();
    assert_eq!(expr, apply(apply(identifier(~":"), number(1)), apply(apply(identifier(~":"), number(2)), identifier(~"[]"))));
}

#[test]
fn parse_instance_class() {
    let mut parser = Parser::new(
r"class Eq a where
    (==) :: a -> a -> Bool

instance Eq a => Eq [a] where
    (==) xs ys = undefined".chars());
    let module = parser.module();

    assert_eq!(module.classes[0].name, ~"Eq");
    assert_eq!(module.instances[0].classname, ~"Eq");
    assert_eq!(module.instances[0].constraints[0].class, ~"Eq");
    assert_eq!(module.instances[0].typ, Type::new_op(~"[]", ~[Type::new_var(0)]));
}

#[test]
fn parse_prelude() {
    let path = &Path::new("Prelude.hs");
    let s  = File::open(path).read_to_end();
    let contents : &str = from_utf8(s);
    let mut parser = Parser::new(contents.chars());
    let module = parser.module();

    assert!(module.bindings.iter().any(|bind| bind.name == ~"foldl"));
    assert!(module.bindings.iter().any(|bind| bind.name == ~"id"));
    assert!(module.classes.iter().any(|class| class.name == ~"Eq"));
}

}

# Neko Object-Oriented Programming (OOP)

Complete reference for classes, traits, inheritance, and runtime behavior in Neko v0.1.

---

## Overview

Neko has **two ways to group data**:

| Feature | Keyword | Methods | Inheritance | Use case |
|---------|---------|---------|-------------|----------|
| **Record** | `struct` | No | No | Plain data (JSON-like records) |
| **Class** | `class` | Yes (instance + static) | Single parent via `extends` | OOP with behavior and polymorphism |

**Traits** (`trait` / `implements`) define method contracts. Classes must implement all trait methods at registration time (runtime check).

Both the **bytecode VM** (default) and the **interpreter** (`--mode interp`) support OOP. Run examples with:

```bash
neko run examples/oop_basics.neko
neko run examples/oop_inheritance.neko
neko run examples/oop_traits.neko
```

---

## Keywords

| Keyword | Purpose |
|---------|---------|
| `class` | Define a class with fields and methods |
| `trait` | Define a method contract (signatures only) |
| `extends` | Single inheritance — one parent class |
| `implements` | Adopt one or more traits |
| `self` | First parameter of instance methods (required) |
| `super` | Call a parent class method from a subclass method |
| `static` | Static field (`static let`) or static method (`static fn`) |
| `public` | Member visible everywhere (default) |
| `private` | Member only accessible inside the defining class body |

> **Note:** `implements` is a keyword, so the runtime trait-check builtin is named **`has_trait`**, not `implements`.

---

## Structs (data-only)

Structs are unchanged from pre-OOP Neko. They create plain `Value::Object` maps — no methods, no vtable.

```neko
struct User {
    name: string
    age: int
}

fn main() {
    let user = User { name: "Alice", age: 30 }
    print(user.name)
}
```

See `examples/structs.neko`.

---

## Classes

### Syntax

```neko
class ClassName [extends ParentName] [implements TraitA, TraitB] {
    [public | private] field_name: type;

    [public | private] static let name [= expr];
    [public | private] static fn name(...) -> Type { ... }

    [public | private] fn method(self, ...) -> Type { ... }
}
```

### Minimal example

```neko
class Counter {
    value: int

    static fn new(start: int) -> Counter {
        return Counter { value: start }
    }

    fn inc(self) {
        self.value = self.value + 1
    }

    fn get(self) -> int {
        return self.value
    }
}

fn main() {
    let c = Counter.new(0)
    c.inc()
    print(c.get())   // 2 after two inc() calls
}
```

See `examples/oop_basics.neko`.

### Instance methods and `self`

- Every **instance method** must have **`self` as its first parameter**.
- On call, `self` is bound automatically: `obj.method(args)` → method receives `(self, args...)`.
- Inside the method body, `self.field` reads/writes instance fields.

### Construction

Two patterns:

**1. Static constructor** — `ClassName.new(...)` calls `static fn new`:

```neko
let c = Counter.new(0)
```

**2. Field initializer** — `ClassName { field: value, ... }`:

```neko
let d = Dog { name: "Rex" }
```

Field names must exist on the class (including inherited fields). Unknown fields → **E1010**.

> **Inherited static `new`:** If only the parent defines `static fn new`, `Child.new(...)` may construct a parent instance. Prefer `Child { ... }` field-init or define `static fn new` on the child.

### Static members

Access via the class name, not an instance:

```neko
Counter.new(0)        // static method
ClassName.static_field // static let (if defined)
```

Static methods are compiled as `__CS__ClassName__method`. Instance methods use `__C__ClassName__method` internally.

### Field and method access

```neko
instance.field        // read field
instance.field = val  // write field
instance.method(args) // dispatch instance method
```

---

## Inheritance (`extends`)

- **Single inheritance only** — one `extends Parent` per class.
- Child classes **inherit parent fields** (merged into the field layout).
- Child **overrides** parent methods by defining a method with the same name.
- Parent **static methods** are copied into the child’s static table at registration.

### `super`

Inside a subclass instance method, call the parent implementation:

```neko
class Animal {
    name: string

    fn speak(self) {
        print("animal")
    }
}

class Dog extends Animal {
    fn speak(self) {
        super.speak()   // calls Animal.speak
        print("woof")
    }
}
```

Rules:

- `super.method()` is only valid **inside an instance method** of a class that has a parent.
- Resolves the **parent class’s** method for the current call context (the class whose method is running).
- `self` must be in scope (first parameter).

See `examples/oop_inheritance.neko`.

---

## Traits (`trait` / `implements`)

### Defining a trait

Traits contain **method signatures only** (no bodies):

```neko
trait Greeter {
    fn greet(self)
}

trait Named {
    fn label(self) -> string
}
```

### Implementing a trait

```neko
class Person implements Greeter {
    name: string

    static fn new(name: string) -> Person {
        return Person { name: name }
    }

    fn greet(self) {
        print("Hello, " + self.name)
    }
}
```

At **class registration**, Neko checks that every trait method exists on the class. Missing methods → **E1022**.

Multiple traits:

```neko
class Worker implements Greeter, Named {
    ...
}
```

### Runtime trait check: `has_trait`

```neko
has_trait(instance, "TraitName")   // returns bool
```

- Returns `true` if the instance’s class (or an ancestor) **implements** that trait name.
- Useful until static type checking lands in v0.2.
- Trait names in type annotations (e.g. `fn f(g: Greeter)`) are **parsed but not enforced** yet.

See `examples/oop_traits.neko`.

---

## Visibility (`public` / `private`)

| Visibility | Default | Rule |
|------------|---------|------|
| `public` | Yes | Fields and methods accessible from anywhere |
| `private` | No | Only accessible from code **inside the defining class body** |

Violations → **E1024**.

Applies to fields, instance methods, static methods, and static fields.

---

## Values and introspection

### Instance vs plain object

| Kind | Runtime type | `type(x)` |
|------|--------------|-----------|
| Class instance | `Value::Instance { class_name, fields }` | class name (e.g. `"Dog"`) |
| Struct / object literal | `Value::Object` map | `"object"` |
| Array | `Value::Array` | `"array"` |

### Builtins

| Builtin | Description |
|---------|-------------|
| `type(x)` | Type name string; instances return their class name |
| `has_trait(x, "TraitName")` | `true` if instance’s class implements the trait |

---

## Modules and `import`

Modules can export **functions, classes, and traits** alongside each other.

- `import "other.neko"` loads the file and merges exported classes/traits into the class registry.
- Same circular-import handling as functions applies.

Top-level in a file can mix `fn`, `struct`, `class`, and `trait` definitions.

---

## Error codes (OOP)

| Code | Meaning |
|------|---------|
| **E1010** | Unknown field in struct/class initializer |
| **E1020** | Unknown class |
| **E1021** | Unknown method on class/instance |
| **E1022** | Trait not implemented (or unknown trait) |
| **E1023** | Invalid `super` call (not in method, no parent, etc.) |
| **E1024** | Private member access |
| **E1025** | Static call on instance / instance call on static |

Full catalog: [ERRORS.md](ERRORS.md).

---

## Execution model

### Interpreter

1. Parse program → register traits, then classes (parents before children).
2. `ClassRegistry` builds merged fields, method maps, static tables.
3. `obj.method(args)` → lookup method on instance’s class vtable; prepend `self`.
4. `super.method()` → `resolve_super_method` on parent; `MethodContext` tracks current class.
5. `Class.member` → static dispatch from class table.

### VM (bytecode)

Class metadata is embedded in the bytecode module (`classes`, `traits`, `field_names`).

| Opcode | Purpose |
|--------|---------|
| `MakeInstance(class, n)` | `ClassName { ... }` field initialization |
| `MakeObject(n)` | Anonymous `{ key: val, ... }` |
| `GetField(idx)` | Read field on `Instance` or `Object` |
| `SetField(idx)` | Write field on `Instance` or `Object` |
| `CallMethod(field, argc)` | `obj.method(args)` — auto-`self` |
| `CallSuper(method, argc)` | `super.method(args)` — parent dispatch |
| `Call` → `__CS__...` | Static method calls (lowered from `Class.staticFn`) |

Method names are stored in the module `field_names` pool (fixes prior `GetField(0)` name bugs).

`BYTECODE_CACHE_VERSION` is **5** (includes `CallSuper`). Stale `.nekobc` files recompile automatically.

### Dispatch diagram

```
obj.method(args)
    → receiver is Value::Instance
    → mangled __C__{class_name}__{method}
    → enter frame with self in slot 0

super.method(args)
    → current class from frame (__C__Dog__speak → Dog)
    → parent = Dog.extends (Animal)
    → call __C__Animal__method with self + args
```

---

## Grammar (EBNF summary)

See [grammar.ebnf](grammar.ebnf) for the full grammar. Core OOP productions:

```
top_level      = fn_def | struct_def | class_def | trait_def | ... ;

class_def      = "class" ident [ "extends" ident ]
                 { "implements" ident { "," ident } }
                 "{" { class_member } "}" ;

trait_def      = "trait" ident "{" { method_sig } "}" ;

class_member   = [ "public" | "private" ]
                 ( field_def | "static" ( fn_def | static_field ) | fn_def ) ;

class_init     = ident "{" [ field_init { "," field_init } ] "}" ;
```

Postfix `super` is parsed as `super.method(args)` → `SuperCall` AST node.

---

## Examples and tests

| Path | What it demonstrates |
|------|---------------------|
| `examples/oop_basics.neko` | Class, `self`, static `new`, methods |
| `examples/oop_inheritance.neko` | `extends`, `super`, override |
| `examples/oop_traits.neko` | `trait`, `implements`, `has_trait` |
| `examples/structs.neko` | Data-only structs (non-OOP) |
| `tests/oop_classes.neko` | Class + static `new` + field mutation |
| `tests/oop_traits.neko` | Trait implementation + `has_trait` |
| `tests/oop_vm.neko` | Inheritance + traits on VM path |

Run tests:

```bash
neko test tests/oop_classes.neko
cargo test -p neko_cli oop
```

---

## Design decisions

| Decision | Choice |
|----------|--------|
| `struct` vs `class` | Structs stay data-only; classes own OOP |
| Trait keyword | `trait` (Rust-style), adoption via `implements` |
| Inheritance | Single parent; traits for extra contracts |
| `self` | Explicit first parameter (no implicit `this`) |
| Dispatch | Vtable built at class registration (no prototype walk) |
| Typing | Annotations parsed; enforcement deferred to v0.2 |

See also [DECISIONS.md](DECISIONS.md).

---

## Quick reference cheat sheet

```neko
// Data record
struct Point { x: int, y: int }
let p = Point { x: 1, y: 2 }

// Class
class Box {
    items: int
    static fn new(n: int) -> Box { return Box { items: n } }
    fn add(self, n: int) { self.items = self.items + n }
}
let b = Box.new(0)
b.add(1)

// Inheritance
class Child extends Parent {
    fn work(self) { super.work() }
}

// Trait
trait Printable { fn print(self) }
class Doc implements Printable {
    fn print(self) { print("doc") }
}
assert(has_trait(doc, "Printable"))

// Access
public fn open(self) { }
private secret: int
```

---

## Limitations (v0.1)

- No multiple inheritance (use traits for extra interfaces).
- No abstract classes; traits are the contract mechanism.
- Type annotations on parameters (e.g. `fn f(x: Greeter)`) are not enforced.
- Private access is runtime-checked in the interpreter; VM focuses on dispatch correctness.
- Gradual typing: field/method types are documentation until the v0.2 type checker.

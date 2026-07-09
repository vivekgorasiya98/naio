use crate::{Environment, FunctionValue, Value, ValueRef};
use niao_ast::{
    ClassDef, ClassMember, FnDef, MethodSig, TraitDef, TypeName, Visibility,
};
use niao_errors::{NiaoResult, RuntimeError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct InstanceValue {
    pub class_name: String,
    pub fields: HashMap<String, ValueRef>,
}

#[derive(Clone)]
pub struct InstanceMethod {
    pub func: FunctionValue,
    pub visibility: Visibility,
    pub defining_class: String,
}

#[derive(Clone)]
pub struct RuntimeClass {
    pub name: String,
    pub parent: Option<String>,
    pub traits: Vec<String>,
    /// field name -> (type, visibility, defining class)
    pub fields: HashMap<String, (TypeName, Visibility, String)>,
    pub methods: HashMap<String, InstanceMethod>,
    pub static_fields: RefCell<HashMap<String, ValueRef>>,
    pub static_methods: HashMap<String, FunctionValue>,
}

#[derive(Clone)]
pub struct RuntimeTrait {
    pub name: String,
    pub methods: Vec<MethodSig>,
}

/// Tracks the class whose instance method is currently executing (for `super`).
#[derive(Clone, Debug)]
pub struct MethodContext {
    pub class_name: String,
}

thread_local! {
    static CLASS_REGISTRY: RefCell<Option<Rc<RefCell<ClassRegistry>>>> = RefCell::new(None);
    static METHOD_CONTEXT: RefCell<Vec<MethodContext>> = RefCell::new(Vec::new());
}

pub fn set_class_registry(registry: Rc<RefCell<ClassRegistry>>) {
    CLASS_REGISTRY.with(|r| *r.borrow_mut() = Some(registry));
}

pub fn with_class_registry<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&ClassRegistry) -> R,
{
    CLASS_REGISTRY.with(|r| {
        r.borrow()
            .as_ref()
            .map(|reg| f(&reg.borrow()))
    })
}

pub fn push_method_context(ctx: MethodContext) {
    METHOD_CONTEXT.with(|stack| stack.borrow_mut().push(ctx));
}

pub fn pop_method_context() {
    METHOD_CONTEXT.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub fn current_method_context() -> Option<MethodContext> {
    METHOD_CONTEXT.with(|stack| stack.borrow().last().cloned())
}

pub struct ClassRegistry {
    pub classes: HashMap<String, RuntimeClass>,
    pub traits: HashMap<String, RuntimeTrait>,
    pub class_defs: HashMap<String, ClassDef>,
    pub trait_defs: HashMap<String, TraitDef>,
}

impl ClassRegistry {
    pub fn new() -> Self {
        Self {
            classes: HashMap::new(),
            traits: HashMap::new(),
            class_defs: HashMap::new(),
            trait_defs: HashMap::new(),
        }
    }

    pub fn register_trait(&mut self, def: &TraitDef) -> NiaoResult<()> {
        self.trait_defs.insert(def.name.clone(), def.clone());
        self.traits.insert(
            def.name.clone(),
            RuntimeTrait {
                name: def.name.clone(),
                methods: def.methods.clone(),
            },
        );
        Ok(())
    }

    /// Register class/trait metadata for the VM (trait checks, no callable methods).
    pub fn register_metadata(&mut self, traits: &[TraitDef], classes: &[ClassDef]) {
        for t in traits {
            let _ = self.register_trait(t);
        }
        let mut pending: Vec<&ClassDef> = classes.iter().collect();
        let mut guard = 0usize;
        while !pending.is_empty() && guard <= classes.len() {
            guard += 1;
            let mut i = 0;
            while i < pending.len() {
                let def = pending[i];
                if let Some(parent) = &def.extends {
                    if !self.classes.contains_key(parent) {
                        i += 1;
                        continue;
                    }
                }
                let mut fields = HashMap::new();
                if let Some(parent_name) = &def.extends {
                    if let Some(parent) = self.classes.get(parent_name) {
                        for (name, meta) in &parent.fields {
                            fields.insert(name.clone(), meta.clone());
                        }
                    }
                }
                for member in &def.members {
                    if let ClassMember::Field {
                        name,
                        ty,
                        visibility,
                        ..
                    } = member
                    {
                        fields.insert(
                            name.clone(),
                            (ty.clone(), *visibility, def.name.clone()),
                        );
                    }
                }
                let runtime = RuntimeClass {
                    name: def.name.clone(),
                    parent: def.extends.clone(),
                    traits: def.implements.clone(),
                    fields,
                    methods: HashMap::new(),
                    static_fields: RefCell::new(HashMap::new()),
                    static_methods: HashMap::new(),
                };
                self.classes.insert(def.name.clone(), runtime);
                self.class_defs.insert(def.name.clone(), def.clone());
                pending.remove(i);
            }
        }
    }

    pub fn finalize_class(
        &mut self,
        def: &ClassDef,
        make_fn: &impl Fn(&FnDef, Rc<Environment>) -> FunctionValue,
        globals: Rc<Environment>,
    ) -> NiaoResult<()> {
        let mut fields = HashMap::new();
        let mut methods = HashMap::new();
        let mut static_fields = HashMap::new();
        let mut static_methods = HashMap::new();

        if let Some(parent_name) = &def.extends {
            if !self.classes.contains_key(parent_name) {
                return Err(RuntimeError::at(
                    def.span,
                    1020,
                    format!("unknown parent class '{parent_name}'"),
                ));
            }
            let parent = self.classes.get(parent_name).unwrap();
            for (name, (ty, vis, owner)) in &parent.fields {
                fields.insert(name.clone(), (ty.clone(), *vis, owner.clone()));
            }
            for (name, method) in &parent.methods {
                methods.insert(name.clone(), method.clone());
            }
            for (name, method) in &parent.static_methods {
                static_methods.insert(name.clone(), method.clone());
            }
        }

        for member in &def.members {
            match member {
                ClassMember::Field {
                    name,
                    ty,
                    visibility,
                    ..
                } => {
                    fields.insert(
                        name.clone(),
                        (ty.clone(), *visibility, def.name.clone()),
                    );
                }
                ClassMember::Method { def: mdef, visibility } => {
                    if mdef.params.first().map(|p| p.name.as_str()) != Some("self") {
                        return Err(RuntimeError::at(
                            mdef.span,
                            1021,
                            format!(
                                "instance method '{}' must have 'self' as first parameter",
                                mdef.name
                            ),
                        ));
                    }
                    methods.insert(
                        mdef.name.clone(),
                        InstanceMethod {
                            func: make_fn(mdef, Rc::clone(&globals)),
                            visibility: *visibility,
                            defining_class: def.name.clone(),
                        },
                    );
                }
                ClassMember::StaticMethod { def: mdef, .. } => {
                    static_methods.insert(
                        mdef.name.clone(),
                        make_fn(mdef, Rc::clone(&globals)),
                    );
                }
                ClassMember::StaticField { name, .. } => {
                    static_fields.insert(name.clone(), Value::Nil.ref_cell());
                }
            }
        }

        let runtime = RuntimeClass {
            name: def.name.clone(),
            parent: def.extends.clone(),
            traits: def.implements.clone(),
            fields,
            methods,
            static_fields: RefCell::new(static_fields),
            static_methods,
        };
        self.classes.insert(def.name.clone(), runtime);
        self.class_defs.insert(def.name.clone(), def.clone());

        for trait_name in &def.implements {
            self.validate_implements(def, trait_name)?;
        }
        Ok(())
    }

    fn validate_implements(&self, class_def: &ClassDef, trait_name: &str) -> NiaoResult<()> {
        let Some(trait_def) = self.traits.get(trait_name) else {
            return Err(RuntimeError::at(
                class_def.span,
                1022,
                format!("unknown trait '{trait_name}'"),
            ));
        };
        let class = self.classes.get(&class_def.name).ok_or_else(|| {
            RuntimeError::at(
                class_def.span,
                1020,
                format!("class '{}' not registered", class_def.name),
            )
        })?;

        for sig in &trait_def.methods {
            if !class.methods.contains_key(&sig.name) {
                return Err(RuntimeError::at(
                    sig.span,
                    1022,
                    format!(
                        "class '{}' does not implement trait method '{}.{}'",
                        class_def.name, trait_name, sig.name
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn get_class(&self, name: &str) -> Option<&RuntimeClass> {
        self.classes.get(name)
    }

    pub fn instance_implements_trait(&self, instance: &InstanceValue, trait_name: &str) -> bool {
        let Some(class) = self.classes.get(&instance.class_name) else {
            return false;
        };
        if class.traits.iter().any(|t| t == trait_name) {
            return true;
        }
        let mut current = class.parent.as_deref();
        while let Some(parent_name) = current {
            if let Some(parent) = self.classes.get(parent_name) {
                if parent.traits.iter().any(|t| t == trait_name) {
                    return true;
                }
                current = parent.parent.as_deref();
            } else {
                break;
            }
        }
        false
    }

    pub fn check_field_access(
        &self,
        class_name: &str,
        field: &str,
        from_class: Option<&str>,
    ) -> NiaoResult<()> {
        let Some(class) = self.classes.get(class_name) else {
            return Ok(());
        };
        let Some((_, vis, owner)) = class.fields.get(field) else {
            return Ok(());
        };
        if *vis == Visibility::Private && from_class != Some(owner.as_str()) {
            return Err(RuntimeError::at(
                niao_ast::Span::dummy(),
                1024,
                format!("cannot access private field '{field}'"),
            ));
        }
        Ok(())
    }

    pub fn check_method_access(
        &self,
        method: &InstanceMethod,
        from_class: Option<&str>,
    ) -> NiaoResult<()> {
        if method.visibility == Visibility::Private
            && from_class != Some(method.defining_class.as_str())
        {
            return Err(RuntimeError::at(
                niao_ast::Span::dummy(),
                1024,
                format!("cannot access private method '{}'", method.func.def.name),
            ));
        }
        Ok(())
    }

    pub fn resolve_super_method(
        &self,
        class_name: &str,
        method: &str,
    ) -> NiaoResult<InstanceMethod> {
        let Some(class) = self.classes.get(class_name) else {
            return Err(RuntimeError::at(
                niao_ast::Span::dummy(),
                1023,
                "invalid super call: unknown class",
            ));
        };
        let parent_name = class.parent.as_ref().ok_or_else(|| {
            RuntimeError::at(
                niao_ast::Span::dummy(),
                1023,
                format!("invalid super call: '{}' has no parent", class.name),
            )
        })?;
        let parent = self.classes.get(parent_name).ok_or_else(|| {
            RuntimeError::at(
                niao_ast::Span::dummy(),
                1023,
                format!("unknown parent class '{parent_name}'"),
            )
        })?;
        parent.methods.get(method).cloned().ok_or_else(|| {
            RuntimeError::at(
                niao_ast::Span::dummy(),
                1021,
                format!("parent class has no method '{method}'"),
            )
        })
    }
}

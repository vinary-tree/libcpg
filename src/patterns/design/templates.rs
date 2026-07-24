//! GoF pattern templates as CPG subgraphs.
//!
//! This module defines structural templates for each Gang-of-Four design pattern.
//! These templates are used by the VF2 matcher to detect pattern occurrences in CPGs.

use std::sync::Arc;
use smallvec::SmallVec;

use crate::{
    CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, EdgeId, Language,
    MethodSignature, NodeId, SourceRange, Visibility,
};
use crate::pattern::{
    EdgeConstraint, EdgeKindMatcher, NodeConstraint, NodeKindMatcher, NodeKindTag, PatternTemplate,
};
use super::GofPattern;

/// Builds a pattern template CPG for a given GoF pattern.
pub fn build_pattern_cpg(pattern: GofPattern) -> CodePropertyGraph {
    match pattern {
        GofPattern::Singleton => build_singleton_pattern(),
        GofPattern::FactoryMethod => build_factory_method_pattern(),
        GofPattern::AbstractFactory => build_abstract_factory_pattern(),
        GofPattern::Builder => build_builder_pattern(),
        GofPattern::Prototype => build_prototype_pattern(),
        GofPattern::Adapter => build_adapter_pattern(),
        GofPattern::Bridge => build_bridge_pattern(),
        GofPattern::Composite => build_composite_pattern(),
        GofPattern::Decorator => build_decorator_pattern(),
        GofPattern::Facade => build_facade_pattern(),
        GofPattern::Flyweight => build_flyweight_pattern(),
        GofPattern::Proxy => build_proxy_pattern(),
        GofPattern::ChainOfResponsibility => build_chain_of_responsibility_pattern(),
        GofPattern::Command => build_command_pattern(),
        GofPattern::Interpreter => build_interpreter_pattern(),
        GofPattern::Iterator => build_iterator_pattern(),
        GofPattern::Mediator => build_mediator_pattern(),
        GofPattern::Memento => build_memento_pattern(),
        GofPattern::Observer => build_observer_pattern(),
        GofPattern::State => build_state_pattern(),
        GofPattern::Strategy => build_strategy_pattern(),
        GofPattern::TemplateMethod => build_template_method_pattern(),
        GofPattern::Visitor => build_visitor_pattern(),
    }
}

/// Creates a PatternTemplate for a given GoF pattern.
pub fn build_pattern_template(pattern: GofPattern) -> PatternTemplate {
    match pattern {
        GofPattern::Singleton => singleton_template(),
        GofPattern::FactoryMethod => factory_method_template(),
        GofPattern::AbstractFactory => abstract_factory_template(),
        GofPattern::Builder => builder_template(),
        GofPattern::Prototype => prototype_template(),
        GofPattern::Adapter => adapter_template(),
        GofPattern::Bridge => bridge_template(),
        GofPattern::Composite => composite_template(),
        GofPattern::Decorator => decorator_template(),
        GofPattern::Facade => facade_template(),
        GofPattern::Flyweight => flyweight_template(),
        GofPattern::Proxy => proxy_template(),
        GofPattern::ChainOfResponsibility => chain_of_responsibility_template(),
        GofPattern::Command => command_template(),
        GofPattern::Interpreter => interpreter_template(),
        GofPattern::Iterator => iterator_template(),
        GofPattern::Mediator => mediator_template(),
        GofPattern::Memento => memento_template(),
        GofPattern::Observer => observer_template(),
        GofPattern::State => state_template(),
        GofPattern::Strategy => strategy_template(),
        GofPattern::TemplateMethod => template_method_template(),
        GofPattern::Visitor => visitor_template(),
    }
}

/// Helper to create a method signature.
fn make_method(name: &str, is_static: bool) -> MethodSignature {
    MethodSignature {
        name: Arc::from(name),
        params: SmallVec::new(),
        return_type: None,
        is_static,
        is_async: false,
        visibility: Visibility::Public,
    }
}

// ===========================================================================
// Singleton Pattern
// ===========================================================================

fn build_singleton_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: The Singleton class
    let class_id = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Singleton"),
            is_abstract: false,
        },
        range,
    ));

    // Node 1: Static instance field
    let field_id = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Field {
            name: Arc::from("instance"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 2: Private constructor
    let constructor_id = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("new", false),
        },
        range,
    ));

    // Node 3: getInstance method (static)
    let get_instance_id = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("getInstance", true),
        },
        range,
    ));

    // Node 4: Return statement in getInstance
    let return_id = cpg.add_node(CpgNode::new(NodeId::new(4), CpgNodeKind::Return, range));

    // AST edges: class contains field, constructor, and getInstance
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), class_id, field_id, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), class_id, constructor_id, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), class_id, get_instance_id, CpgEdgeKind::AstChild));

    // getInstance contains return
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), get_instance_id, return_id, CpgEdgeKind::AstChild));

    // DFG: return reads from field
    cpg.add_edge(CpgEdge::new(
        EdgeId::new(4),
        field_id,
        return_id,
        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
    ));

    cpg
}

fn singleton_template() -> PatternTemplate {
    // Node 4 (the `return` inside `getInstance`) mirrors `build_singleton_pattern`
    // so `node_constraints.len()` equals the pattern CPG's `node_count()` (5).
    // VF2 emits only complete mappings, so a full match then scores
    // `completeness == 1.0` (confidence == base), never > 1.0.
    PatternTemplate::new("Singleton", "Class with private constructor and single static instance")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Return)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(3, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

// ===========================================================================
// Factory Method Pattern
// ===========================================================================

fn build_factory_method_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Abstract Creator
    let creator_id = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Creator"),
            is_abstract: true,
        },
        range,
    ));

    // Node 1: Factory method
    let factory_method_id = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("createProduct", false),
        },
        range,
    ));

    // Node 2: Concrete Creator
    let concrete_creator_id = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("ConcreteCreator"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: Overridden factory method
    let override_method_id = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("createProduct", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), creator_id, factory_method_id, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), concrete_creator_id, override_method_id, CpgEdgeKind::AstChild));

    // Inheritance edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), concrete_creator_id, creator_id, CpgEdgeKind::Inherits));

    cpg
}

fn factory_method_template() -> PatternTemplate {
    PatternTemplate::new("Factory Method", "Abstract factory method overridden by concrete creators")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Abstract Factory Pattern
// ===========================================================================

fn build_abstract_factory_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Abstract Factory trait
    let factory_trait = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("AbstractFactory"),
        },
        range,
    ));

    // Node 1: createProductA method
    let create_a = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("createProductA", false),
        },
        range,
    ));

    // Node 2: createProductB method
    let create_b = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("createProductB", false),
        },
        range,
    ));

    // Node 3: Concrete Factory
    let concrete_factory = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Class {
            name: Arc::from("ConcreteFactory"),
            is_abstract: false,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), factory_trait, create_a, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), factory_trait, create_b, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), concrete_factory, factory_trait, CpgEdgeKind::Implements));

    cpg
}

fn abstract_factory_template() -> PatternTemplate {
    PatternTemplate::new("Abstract Factory", "Factory interface with multiple creation methods")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Builder Pattern
// ===========================================================================

fn build_builder_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Builder class
    let builder = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Builder"),
            is_abstract: false,
        },
        range,
    ));

    // Node 1: setPartA method (returns Self)
    let set_a = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("setPartA", false),
        },
        range,
    ));

    // Node 2: setPartB method (returns Self)
    let set_b = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("setPartB", false),
        },
        range,
    ));

    // Node 3: build method
    let build = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("build", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), builder, set_a, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), builder, set_b, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), builder, build, CpgEdgeKind::AstChild));

    cpg
}

fn builder_template() -> PatternTemplate {
    PatternTemplate::new("Builder", "Fluent builder with chained setter methods and build()")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Prototype Pattern
// ===========================================================================

fn build_prototype_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Prototype trait
    let prototype = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Prototype"),
        },
        range,
    ));

    // Node 1: clone method
    let clone = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("clone", false),
        },
        range,
    ));

    // Node 2: Concrete Prototype
    let concrete = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("ConcretePrototype"),
            is_abstract: false,
        },
        range,
    ));

    // AST edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), prototype, clone, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), concrete, prototype, CpgEdgeKind::Implements));

    cpg
}

fn prototype_template() -> PatternTemplate {
    PatternTemplate::new("Prototype", "Interface with clone() method implemented by concrete types")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(
            NodeConstraint::new(1)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("clone"),
        )
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

// ===========================================================================
// Adapter Pattern
// ===========================================================================

fn build_adapter_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Target trait
    let target = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Target"),
        },
        range,
    ));

    // Node 1: Adapter class
    let adapter = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Class {
            name: Arc::from("Adapter"),
            is_abstract: false,
        },
        range,
    ));

    // Node 2: Adaptee field
    let adaptee_field = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Field {
            name: Arc::from("adaptee"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 3: request method that delegates
    let request = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("request", false),
        },
        range,
    ));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), adapter, target, CpgEdgeKind::Implements));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), adapter, adaptee_field, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), adapter, request, CpgEdgeKind::AstChild));

    cpg
}

fn adapter_template() -> PatternTemplate {
    PatternTemplate::new("Adapter", "Class implementing target interface and delegating to adaptee")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(1, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(1, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Bridge Pattern
// ===========================================================================

fn build_bridge_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Abstraction class
    let abstraction = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Abstraction"),
            is_abstract: true,
        },
        range,
    ));

    // Node 1: Implementor trait (bridge)
    let implementor = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Trait {
            name: Arc::from("Implementor"),
        },
        range,
    ));

    // Node 2: Field holding implementor
    let impl_field = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Field {
            name: Arc::from("impl"),
            field_type: None,
            visibility: Visibility::Protected,
        },
        range,
    ));

    // Node 3: operation method
    let operation = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("operation", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), abstraction, impl_field, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), abstraction, operation, CpgEdgeKind::AstChild));

    // Type reference from field to implementor type
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), impl_field, implementor, CpgEdgeKind::TypeOf));

    cpg
}

fn bridge_template() -> PatternTemplate {
    // Node 3 (the abstraction's `operation` method) mirrors `build_bridge_pattern`
    // so `node_constraints.len()` matches the pattern CPG's `node_count()` (4).
    PatternTemplate::new("Bridge", "Abstraction with field referencing Implementor interface")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Composite Pattern
// ===========================================================================

fn build_composite_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Component trait
    let component = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Component"),
        },
        range,
    ));

    // Node 1: operation method
    let operation = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("operation", false),
        },
        range,
    ));

    // Node 2: Composite class
    let composite = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("Composite"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: children field (collection of Component)
    let children = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("children"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 4: add method
    let add = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Function {
            signature: make_method("add", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), component, operation, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), composite, children, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), composite, add, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), composite, component, CpgEdgeKind::Implements));

    cpg
}

fn composite_template() -> PatternTemplate {
    // Node 4 (the composite's `add` method) mirrors `build_composite_pattern`
    // so `node_constraints.len()` matches the pattern CPG's `node_count()` (5).
    PatternTemplate::new("Composite", "Component interface with Composite holding children collection")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Decorator Pattern
// ===========================================================================

fn build_decorator_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Component trait
    let component = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Component"),
        },
        range,
    ));

    // Node 1: Decorator class (implements Component, wraps Component)
    let decorator = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Class {
            name: Arc::from("Decorator"),
            is_abstract: false,
        },
        range,
    ));

    // Node 2: wrapped component field
    let wrapped = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Field {
            name: Arc::from("component"),
            field_type: None,
            visibility: Visibility::Protected,
        },
        range,
    ));

    // Node 3: operation method
    let operation = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("operation", false),
        },
        range,
    ));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), decorator, component, CpgEdgeKind::Implements));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), decorator, wrapped, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), decorator, operation, CpgEdgeKind::AstChild));

    // Type reference: wrapped field is of type Component
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), wrapped, component, CpgEdgeKind::TypeOf));

    cpg
}

fn decorator_template() -> PatternTemplate {
    PatternTemplate::new("Decorator", "Class implementing Component, wrapping and delegating to Component")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(1, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(1, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Facade Pattern
// ===========================================================================

fn build_facade_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Facade class
    let facade = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Facade"),
            is_abstract: false,
        },
        range,
    ));

    // Node 1-2: Multiple subsystem fields
    let subsystem1 = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Field {
            name: Arc::from("subsystem1"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));
    let subsystem2 = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Field {
            name: Arc::from("subsystem2"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 3: simplified operation
    let operation = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("operation", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), facade, subsystem1, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), facade, subsystem2, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), facade, operation, CpgEdgeKind::AstChild));

    cpg
}

fn facade_template() -> PatternTemplate {
    PatternTemplate::new("Facade", "Class with multiple subsystem fields and simplified methods")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.6)
}

// ===========================================================================
// Flyweight Pattern
// ===========================================================================

fn build_flyweight_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Flyweight trait
    let flyweight = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Flyweight"),
        },
        range,
    ));

    // Node 1: operation with extrinsic state parameter
    let operation = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("operation", false),
        },
        range,
    ));

    // Node 2: FlyweightFactory class
    let factory = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("FlyweightFactory"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: cache field
    let cache = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("flyweights"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 4: getFlyweight method
    let get_flyweight = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Function {
            signature: make_method("getFlyweight", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), flyweight, operation, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), factory, cache, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), factory, get_flyweight, CpgEdgeKind::AstChild));

    cpg
}

fn flyweight_template() -> PatternTemplate {
    // Node 4 (the factory's `getFlyweight` method) mirrors `build_flyweight_pattern`
    // so `node_constraints.len()` matches the pattern CPG's `node_count()` (5).
    PatternTemplate::new("Flyweight", "Factory with cache returning shared Flyweight instances")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Proxy Pattern
// ===========================================================================

fn build_proxy_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Subject trait
    let subject = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Subject"),
        },
        range,
    ));

    // Node 1: request method
    let request = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("request", false),
        },
        range,
    ));

    // Node 2: Proxy class
    let proxy = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("Proxy"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: real subject field
    let real_subject = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("realSubject"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), subject, request, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), proxy, real_subject, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), proxy, subject, CpgEdgeKind::Implements));

    cpg
}

fn proxy_template() -> PatternTemplate {
    PatternTemplate::new("Proxy", "Class implementing Subject interface, holding real Subject reference")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Chain of Responsibility Pattern
// ===========================================================================

fn build_chain_of_responsibility_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Handler trait
    let handler = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Handler"),
        },
        range,
    ));

    // Node 1: handle method
    let handle = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("handle", false),
        },
        range,
    ));

    // Node 2: setNext method
    let set_next = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("setNext", false),
        },
        range,
    ));

    // Node 3: ConcreteHandler
    let concrete = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Class {
            name: Arc::from("ConcreteHandler"),
            is_abstract: false,
        },
        range,
    ));

    // Node 4: next field
    let next = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Field {
            name: Arc::from("next"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), handler, handle, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), handler, set_next, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), concrete, next, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), concrete, handler, CpgEdgeKind::Implements));

    cpg
}

fn chain_of_responsibility_template() -> PatternTemplate {
    PatternTemplate::new("Chain of Responsibility", "Handler with next handler field forming a chain")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(3, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Command Pattern
// ===========================================================================

fn build_command_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Command trait
    let command = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Command"),
        },
        range,
    ));

    // Node 1: execute method
    let execute = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("execute", false),
        },
        range,
    ));

    // Node 2: ConcreteCommand
    let concrete = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("ConcreteCommand"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: receiver field
    let receiver = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("receiver"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), command, execute, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), concrete, receiver, CpgEdgeKind::AstChild));

    // Implements edge
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), concrete, command, CpgEdgeKind::Implements));

    cpg
}

fn command_template() -> PatternTemplate {
    PatternTemplate::new("Command", "Command interface with execute(), ConcreteCommand with receiver")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Interpreter Pattern
// ===========================================================================

fn build_interpreter_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Expression trait
    let expression = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Expression"),
        },
        range,
    ));

    // Node 1: interpret method
    let interpret = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("interpret", false),
        },
        range,
    ));

    // Node 2: TerminalExpression
    let terminal = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("TerminalExpression"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: NonTerminalExpression
    let non_terminal = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Class {
            name: Arc::from("NonTerminalExpression"),
            is_abstract: false,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), expression, interpret, CpgEdgeKind::AstChild));

    // Implements edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), terminal, expression, CpgEdgeKind::Implements));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), non_terminal, expression, CpgEdgeKind::Implements));

    cpg
}

fn interpreter_template() -> PatternTemplate {
    PatternTemplate::new("Interpreter", "Expression interface with Terminal and NonTerminal implementations")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(
            NodeConstraint::new(1)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("interpret"),
        )
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        // Node 3 (NonTerminalExpression) mirrors `build_interpreter_pattern` so
        // `node_constraints.len()` matches the pattern CPG's `node_count()` (4).
        // Like node 2 (TerminalExpression), it is related to the interface only
        // by an `Implements` edge, which the template's AnyAst edges do not
        // constrain — so it carries a node constraint but no edge constraint.
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Iterator Pattern
// ===========================================================================

fn build_iterator_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Iterator trait
    let iterator = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Iterator"),
        },
        range,
    ));

    // Node 1: next method
    let next = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("next", false),
        },
        range,
    ));

    // Node 2: hasNext method
    let has_next = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("hasNext", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), iterator, next, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), iterator, has_next, CpgEdgeKind::AstChild));

    cpg
}

fn iterator_template() -> PatternTemplate {
    PatternTemplate::new("Iterator", "Iterator trait with next/hasNext methods")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(
            NodeConstraint::new(1)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("next"),
        )
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

// ===========================================================================
// Mediator Pattern
// ===========================================================================

fn build_mediator_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Mediator trait
    let mediator = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Mediator"),
        },
        range,
    ));

    // Node 1: notify method
    let notify = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("notify", false),
        },
        range,
    ));

    // Node 2: Colleague class
    let colleague = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("Colleague"),
            is_abstract: true,
        },
        range,
    ));

    // Node 3: mediator field
    let med_field = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("mediator"),
            field_type: None,
            visibility: Visibility::Protected,
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), mediator, notify, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), colleague, med_field, CpgEdgeKind::AstChild));

    // Type reference
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), med_field, mediator, CpgEdgeKind::TypeOf));

    cpg
}

fn mediator_template() -> PatternTemplate {
    PatternTemplate::new("Mediator", "Mediator interface with notify(), Colleagues holding Mediator reference")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Memento Pattern
// ===========================================================================

fn build_memento_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Originator class
    let originator = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Originator"),
            is_abstract: false,
        },
        range,
    ));

    // Node 1: state field
    let state = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Field {
            name: Arc::from("state"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 2: save method
    let save = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("save", false),
        },
        range,
    ));

    // Node 3: restore method
    let restore = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("restore", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), originator, state, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), originator, save, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), originator, restore, CpgEdgeKind::AstChild));

    cpg
}

fn memento_template() -> PatternTemplate {
    PatternTemplate::new("Memento", "Originator with save/restore methods and state field")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(
            NodeConstraint::new(2)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("save"),
        )
        .with_node(
            NodeConstraint::new(3)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("restore"),
        )
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

// ===========================================================================
// Observer Pattern
// ===========================================================================

fn build_observer_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Subject class
    let subject = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("Subject"),
            is_abstract: false,
        },
        range,
    ));

    // Node 1: observers field (collection)
    let observers = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Field {
            name: Arc::from("observers"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 2: attach method
    let attach = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("attach", false),
        },
        range,
    ));

    // Node 3: notify method
    let notify = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("notify", false),
        },
        range,
    ));

    // Node 4: Observer trait
    let observer = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Trait {
            name: Arc::from("Observer"),
        },
        range,
    ));

    // Node 5: update method
    let update = cpg.add_node(CpgNode::new(
        NodeId::new(5),
        CpgNodeKind::Function {
            signature: make_method("update", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), subject, observers, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), subject, attach, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), subject, notify, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), observer, update, CpgEdgeKind::AstChild));

    cpg
}

fn observer_template() -> PatternTemplate {
    PatternTemplate::new("Observer", "Subject with observers collection and attach/notify methods")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(5).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(4, 5).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

// ===========================================================================
// State Pattern
// ===========================================================================

fn build_state_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: State trait
    let state = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("State"),
        },
        range,
    ));

    // Node 1: handle method
    let handle = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("handle", false),
        },
        range,
    ));

    // Node 2: Context class
    let context = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("Context"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: state field
    let state_field = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("state"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 4: setState method
    let set_state = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Function {
            signature: make_method("setState", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), state, handle, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), context, state_field, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), context, set_state, CpgEdgeKind::AstChild));

    // Type reference
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), state_field, state, CpgEdgeKind::TypeOf));

    cpg
}

fn state_template() -> PatternTemplate {
    // Node 4 (the context's `setState` method) mirrors `build_state_pattern` so
    // `node_constraints.len()` matches the pattern CPG's `node_count()` (5).
    PatternTemplate::new("State", "Context with State field and setState method")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Strategy Pattern
// ===========================================================================

fn build_strategy_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Strategy trait
    let strategy = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Strategy"),
        },
        range,
    ));

    // Node 1: execute method
    let execute = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("execute", false),
        },
        range,
    ));

    // Node 2: Context class
    let context = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Class {
            name: Arc::from("Context"),
            is_abstract: false,
        },
        range,
    ));

    // Node 3: strategy field
    let strategy_field = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Field {
            name: Arc::from("strategy"),
            field_type: None,
            visibility: Visibility::Private,
        },
        range,
    ));

    // Node 4: setStrategy method
    let set_strategy = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Function {
            signature: make_method("setStrategy", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), strategy, execute, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), context, strategy_field, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), context, set_strategy, CpgEdgeKind::AstChild));

    // Type reference
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), strategy_field, strategy, CpgEdgeKind::TypeOf));

    cpg
}

fn strategy_template() -> PatternTemplate {
    // Node 4 (the context's `setStrategy` method) mirrors `build_strategy_pattern`
    // so `node_constraints.len()` matches the pattern CPG's `node_count()` (5).
    PatternTemplate::new("Strategy", "Context with Strategy field and setStrategy method")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_node(NodeConstraint::new(4).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(2, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Template Method Pattern
// ===========================================================================

fn build_template_method_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Abstract class
    let abstract_class = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: Arc::from("AbstractClass"),
            is_abstract: true,
        },
        range,
    ));

    // Node 1: templateMethod
    let template_method = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("templateMethod", false),
        },
        range,
    ));

    // Node 2: primitiveOperation1 (abstract)
    let prim_op1 = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("primitiveOperation1", false),
        },
        range,
    ));

    // Node 3: primitiveOperation2 (abstract)
    let prim_op2 = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Function {
            signature: make_method("primitiveOperation2", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), abstract_class, template_method, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), abstract_class, prim_op1, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), abstract_class, prim_op2, CpgEdgeKind::AstChild));

    // Call edges from template method to primitive operations
    cpg.add_edge(CpgEdge::new(EdgeId::new(3), template_method, prim_op1, CpgEdgeKind::CallSite));
    cpg.add_edge(CpgEdge::new(EdgeId::new(4), template_method, prim_op2, CpgEdgeKind::CallSite));

    cpg
}

fn template_method_template() -> PatternTemplate {
    PatternTemplate::new("Template Method", "Abstract class with template method calling abstract operations")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 3).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.7)
}

// ===========================================================================
// Visitor Pattern
// ===========================================================================

fn build_visitor_pattern() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let range = SourceRange::default();

    // Node 0: Visitor trait
    let visitor = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Trait {
            name: Arc::from("Visitor"),
        },
        range,
    ));

    // Node 1: visitConcreteElementA
    let visit_a = cpg.add_node(CpgNode::new(
        NodeId::new(1),
        CpgNodeKind::Function {
            signature: make_method("visitConcreteElementA", false),
        },
        range,
    ));

    // Node 2: visitConcreteElementB
    let visit_b = cpg.add_node(CpgNode::new(
        NodeId::new(2),
        CpgNodeKind::Function {
            signature: make_method("visitConcreteElementB", false),
        },
        range,
    ));

    // Node 3: Element trait
    let element = cpg.add_node(CpgNode::new(
        NodeId::new(3),
        CpgNodeKind::Trait {
            name: Arc::from("Element"),
        },
        range,
    ));

    // Node 4: accept method
    let accept = cpg.add_node(CpgNode::new(
        NodeId::new(4),
        CpgNodeKind::Function {
            signature: make_method("accept", false),
        },
        range,
    ));

    // AST edges
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), visitor, visit_a, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(1), visitor, visit_b, CpgEdgeKind::AstChild));
    cpg.add_edge(CpgEdge::new(EdgeId::new(2), element, accept, CpgEdgeKind::AstChild));

    cpg
}

fn visitor_template() -> PatternTemplate {
    PatternTemplate::new("Visitor", "Visitor interface with visit methods, Element with accept")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
        .with_node(NodeConstraint::new(3).with_kind(NodeKindMatcher::Exact(NodeKindTag::Trait)))
        .with_node(
            NodeConstraint::new(4)
                .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                .with_name_pattern("accept"),
        )
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst))
        .with_edge(EdgeConstraint::new(3, 4).with_kind(EdgeKindMatcher::AnyAst))
        .with_min_confidence(0.8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_singleton_pattern() {
        let cpg = build_singleton_pattern();
        assert!(cpg.node_count() > 0);
        assert!(cpg.edge_count() > 0);
    }

    #[test]
    fn test_build_all_patterns() {
        for pattern in [
            GofPattern::Singleton,
            GofPattern::FactoryMethod,
            GofPattern::AbstractFactory,
            GofPattern::Builder,
            GofPattern::Prototype,
            GofPattern::Adapter,
            GofPattern::Bridge,
            GofPattern::Composite,
            GofPattern::Decorator,
            GofPattern::Facade,
            GofPattern::Flyweight,
            GofPattern::Proxy,
            GofPattern::ChainOfResponsibility,
            GofPattern::Command,
            GofPattern::Interpreter,
            GofPattern::Iterator,
            GofPattern::Mediator,
            GofPattern::Memento,
            GofPattern::Observer,
            GofPattern::State,
            GofPattern::Strategy,
            GofPattern::TemplateMethod,
            GofPattern::Visitor,
        ] {
            let cpg = build_pattern_cpg(pattern);
            assert!(
                cpg.node_count() > 0,
                "Pattern {:?} should have nodes",
                pattern
            );
        }
    }

    #[test]
    fn test_pattern_templates() {
        let template = singleton_template();
        assert_eq!(template.name, "Singleton");
        assert!(!template.node_constraints.is_empty());
    }

    /// All 23 GoF patterns, in a single place for exhaustive coverage.
    const ALL_PATTERNS: [GofPattern; 23] = [
        GofPattern::Singleton,
        GofPattern::FactoryMethod,
        GofPattern::AbstractFactory,
        GofPattern::Builder,
        GofPattern::Prototype,
        GofPattern::Adapter,
        GofPattern::Bridge,
        GofPattern::Composite,
        GofPattern::Decorator,
        GofPattern::Facade,
        GofPattern::Flyweight,
        GofPattern::Proxy,
        GofPattern::ChainOfResponsibility,
        GofPattern::Command,
        GofPattern::Interpreter,
        GofPattern::Iterator,
        GofPattern::Mediator,
        GofPattern::Memento,
        GofPattern::Observer,
        GofPattern::State,
        GofPattern::Strategy,
        GofPattern::TemplateMethod,
        GofPattern::Visitor,
    ];

    /// `build_pattern_template` must produce a well-formed template for ALL 23
    /// patterns (the pre-existing suite only exercised Singleton).
    #[test]
    fn test_build_pattern_template_all_23() {
        for pattern in ALL_PATTERNS {
            let template = build_pattern_template(pattern);
            assert!(
                !template.name.is_empty(),
                "{pattern:?}: template name must be non-empty"
            );
            assert!(
                !template.node_constraints.is_empty(),
                "{pattern:?}: template must have node constraints"
            );
            assert!(
                template.min_confidence > 0.0 && template.min_confidence <= 1.0,
                "{pattern:?}: min_confidence {} out of (0, 1]",
                template.min_confidence
            );
            // The node constraint indices are exactly 0..len, matching the
            // pattern CPG's node ids.
            let mut indices: Vec<usize> =
                template.node_constraints.iter().map(|c| c.index).collect();
            indices.sort_unstable();
            let expected: Vec<usize> = (0..template.node_constraints.len()).collect();
            assert_eq!(indices, expected, "{pattern:?}: node indices must be 0..n");
        }
    }

    /// `build_pattern_cpg` must yield a non-empty graph WITH edges for ALL 23
    /// patterns (the pre-existing suite only asserted `edge_count > 0` for
    /// Singleton).
    #[test]
    fn test_build_pattern_cpg_edges_all_23() {
        for pattern in ALL_PATTERNS {
            let cpg = build_pattern_cpg(pattern);
            assert!(cpg.node_count() > 0, "{pattern:?}: expected nodes");
            assert!(cpg.edge_count() > 0, "{pattern:?}: expected edges");
        }
    }

    /// Core invariant restored by the boy-scout template fixes: the pattern CPG
    /// (what VF2 searches with) and the pattern template (what
    /// `calculate_confidence` divides by) describe the same node set, so a
    /// complete VF2 match scores `completeness == 1.0`.
    #[test]
    fn test_template_cpg_node_parity_all_23() {
        for pattern in ALL_PATTERNS {
            let cpg = build_pattern_cpg(pattern);
            let template = build_pattern_template(pattern);
            assert_eq!(
                cpg.node_count(),
                template.node_constraints.len(),
                "{pattern:?}: pattern CPG node_count must equal template node_constraints.len()"
            );
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// All 23 patterns as a proptest strategy.
    fn arb_gof_pattern() -> impl Strategy<Value = GofPattern> {
        prop_oneof![
            Just(GofPattern::Singleton),
            Just(GofPattern::FactoryMethod),
            Just(GofPattern::AbstractFactory),
            Just(GofPattern::Builder),
            Just(GofPattern::Prototype),
            Just(GofPattern::Adapter),
            Just(GofPattern::Bridge),
            Just(GofPattern::Composite),
            Just(GofPattern::Decorator),
            Just(GofPattern::Facade),
            Just(GofPattern::Flyweight),
            Just(GofPattern::Proxy),
            Just(GofPattern::ChainOfResponsibility),
            Just(GofPattern::Command),
            Just(GofPattern::Interpreter),
            Just(GofPattern::Iterator),
            Just(GofPattern::Mediator),
            Just(GofPattern::Memento),
            Just(GofPattern::Observer),
            Just(GofPattern::State),
            Just(GofPattern::Strategy),
            Just(GofPattern::TemplateMethod),
            Just(GofPattern::Visitor),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// For every GoF pattern: the pattern CPG and its template agree on node
        /// count, both counts are positive, and the CPG has at least one edge.
        #[test]
        fn prop_template_cpg_parity(pattern in arb_gof_pattern()) {
            let cpg = build_pattern_cpg(pattern);
            let template = build_pattern_template(pattern);

            prop_assert!(cpg.node_count() > 0, "{:?}: no nodes", pattern);
            prop_assert!(cpg.edge_count() > 0, "{:?}: no edges", pattern);
            prop_assert!(
                !template.node_constraints.is_empty(),
                "{:?}: no node constraints",
                pattern
            );
            prop_assert_eq!(
                cpg.node_count(),
                template.node_constraints.len(),
                "{:?}: node_count != node_constraints.len()",
                pattern
            );
        }
    }
}

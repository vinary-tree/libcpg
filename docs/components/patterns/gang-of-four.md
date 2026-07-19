# Gang of Four Patterns

libcpg provides detection for all 23 Gang of Four design patterns, categorized into Creational, Structural, and Behavioral groups.

## Overview

The Gang of Four (GoF) patterns are classic software design patterns from the book "Design Patterns: Elements of Reusable Object-Oriented Software" (1994). libcpg detects these patterns using subgraph isomorphism on Code Property Graphs.

## Using the Detector

### Basic Usage

```rust
use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

// Create detector
let detector = GofPatternDetector::new();

// Detect all patterns
let matches = detector.detect(&cpg);

for m in matches {
    println!("{}: confidence {:.0}%", m.pattern_name, m.confidence * 100.0);
}
```

### Filtering Patterns

Detect only specific patterns:

```rust
let detector = GofPatternDetector::new()
    .with_patterns(vec![
        GofPattern::Singleton,
        GofPattern::Factory,
        GofPattern::Observer,
    ])
    .with_min_confidence(0.8);

let matches = detector.detect(&cpg);
```

### Accessing Pattern Metadata

```rust
for m in matches {
    // Get pattern category
    let category = m.metadata.get("category").unwrap();  // "Creational", "Structural", or "Behavioral"

    // Get pattern type
    let pattern_type = m.metadata.get("pattern_type").unwrap();  // "GoF"

    println!("{} ({}) - {}", m.pattern_name, category, m.confidence);
}
```

## Creational Patterns

Patterns that deal with object creation mechanisms.

### Singleton

**Intent:** Ensure a class has only one instance with global access.

**Structure:**
```
┌───────────────────────────────────┐
│            Singleton               │
├───────────────────────────────────┤
│ - instance: Singleton             │
├───────────────────────────────────┤
│ - Singleton()      ◄── private    │
│ + getInstance()    ◄── static     │
└───────────────────────────────────┘
```

**Detection criteria:**
- Class with private static field of its own type
- Private constructor
- Public static method returning the instance

**Example code detected:**
```rust
struct Logger {
    // Detection looks for:
    // 1. Private/static instance field
    // 2. Private constructor or restricted initialization
    // 3. Static getInstance/instance method
}

impl Logger {
    fn instance() -> &'static Logger {
        static LOGGER: OnceLock<Logger> = OnceLock::new();
        LOGGER.get_or_init(|| Logger::new())
    }

    fn new() -> Self { /* ... */ }
}
```

### Factory Method

**Intent:** Define an interface for creating objects, letting subclasses decide which class to instantiate.

**Structure:**
```
         ┌─────────────────────┐
         │      Creator        │
         ├─────────────────────┤
         │ + factoryMethod()   │
         │ + operation()       │
         └──────────┬──────────┘
                    │
         ┌──────────┴──────────┐
         │                     │
┌────────▼────────┐  ┌────────▼────────┐
│ ConcreteCreator1│  │ ConcreteCreator2│
├─────────────────┤  ├─────────────────┤
│ factoryMethod() │  │ factoryMethod() │
└─────────────────┘  └─────────────────┘
```

**Detection criteria:**
- Abstract class/trait with method returning a type
- Concrete subclasses overriding the factory method

### Abstract Factory

**Intent:** Provide an interface for creating families of related objects.

**Structure:**
```
┌───────────────────────┐
│   AbstractFactory     │◄───────────┐
├───────────────────────┤            │
│ + createProductA()    │            │
│ + createProductB()    │     ┌──────┴──────┐
└───────────┬───────────┘     │   Client    │
            │                 └─────────────┘
   ┌────────┴────────┐
   ▼                 ▼
Factory1          Factory2
```

### Builder

**Intent:** Separate the construction of a complex object from its representation.

**Detection criteria:**
- Builder class with step-by-step construction methods
- Method chaining (returning self)
- Final `build()` method

**Example code detected:**
```rust
struct QueryBuilder {
    table: String,
    columns: Vec<String>,
    conditions: Vec<String>,
}

impl QueryBuilder {
    fn new(table: &str) -> Self { /* ... */ }
    fn select(mut self, col: &str) -> Self { /* returns self */ }
    fn where_clause(mut self, cond: &str) -> Self { /* returns self */ }
    fn build(self) -> Query { /* ... */ }
}
```

### Prototype

**Intent:** Create new objects by copying existing ones.

**Detection criteria:**
- `clone()` method implementation
- Factory using clone for object creation

## Structural Patterns

Patterns that deal with object composition.

### Adapter

**Intent:** Convert the interface of a class into another interface clients expect.

**Structure:**
```
┌────────────┐        ┌─────────────────┐
│   Client   │───────▶│    Target       │
└────────────┘        │ + request()     │
                      └────────┬────────┘
                               │
                      ┌────────▼────────┐
                      │    Adapter      │────┐
                      │ + request()     │    │
                      └─────────────────┘    │
                               │             │
                      ┌────────▼────────┐    │ delegates
                      │    Adaptee      │◄───┘
                      │ + specificReq() │
                      └─────────────────┘
```

**Detection criteria:**
- Class implementing an interface
- Delegates to another class with different method names

### Bridge

**Intent:** Decouple an abstraction from its implementation.

### Composite

**Intent:** Compose objects into tree structures to represent part-whole hierarchies.

**Detection criteria:**
- Component interface
- Leaf and Composite classes
- Composite contains collection of Components

### Decorator

**Intent:** Attach additional responsibilities to an object dynamically.

**Structure:**
```
┌──────────────────┐
│    Component     │◄──────────────────┐
│ + operation()    │                   │
└────────┬─────────┘                   │
         │                             │
    ┌────┴────┐                        │
    │         │                        │
┌───▼───┐ ┌───▼─────────────┐          │
│Concrete│ │   Decorator     │─────────┘
│Comp.   │ │ + operation()   │ wraps
└────────┘ └────────┬────────┘
                    │
          ┌─────────┴─────────┐
          │                   │
   ┌──────▼──────┐    ┌──────▼──────┐
   │ DecoratorA  │    │ DecoratorB  │
   └─────────────┘    └─────────────┘
```

### Facade

**Intent:** Provide a unified interface to a set of interfaces in a subsystem.

### Flyweight

**Intent:** Use sharing to support large numbers of fine-grained objects efficiently.

### Proxy

**Intent:** Provide a surrogate or placeholder for another object.

## Behavioral Patterns

Patterns that deal with object interaction and responsibility.

### Chain of Responsibility

**Intent:** Avoid coupling the sender of a request to its receiver.

**Detection criteria:**
- Handler interface with `handle()` method
- Handler reference to next handler
- Chain construction

### Command

**Intent:** Encapsulate a request as an object.

**Structure:**
```
┌───────────┐      ┌─────────────┐
│  Invoker  │─────▶│   Command   │
└───────────┘      │ + execute() │
                   └──────┬──────┘
                          │
              ┌───────────┴───────────┐
              │                       │
    ┌─────────▼─────────┐   ┌────────▼────────┐
    │ ConcreteCommand1  │   │ ConcreteCommand2│
    │ - receiver        │   └─────────────────┘
    │ + execute()       │
    └─────────────────────┘
              │
              ▼
    ┌─────────────────────┐
    │     Receiver        │
    │ + action()          │
    └─────────────────────┘
```

### Iterator

**Intent:** Provide a way to access elements of a collection sequentially.

**Detection criteria:**
- Iterator trait/interface implementation
- `next()`, `has_next()` or equivalent methods

### Mediator

**Intent:** Define an object that encapsulates how a set of objects interact.

### Memento

**Intent:** Capture and externalize an object's internal state.

### Observer

**Intent:** Define a one-to-many dependency between objects.

**Structure:**
```
┌─────────────────────┐        ┌─────────────────────┐
│      Subject        │◄───────│     Observer        │
├─────────────────────┤        ├─────────────────────┤
│ - observers: List   │        │ + update()          │
├─────────────────────┤        └──────────┬──────────┘
│ + attach(Observer)  │                   │
│ + detach(Observer)  │        ┌──────────┴──────────┐
│ + notify()          │        │  ConcreteObserver   │
└─────────────────────┘        │ + update()          │
                               └─────────────────────┘
```

**Detection criteria:**
- Subject with list of observers
- `attach()`, `detach()`, `notify()` methods
- Observer interface with `update()` method

### State

**Intent:** Allow an object to alter its behavior when its internal state changes.

**Detection criteria:**
- Context class with State reference
- State interface with behavior methods
- Concrete state classes

### Strategy

**Intent:** Define a family of algorithms, encapsulate each one, and make them interchangeable.

**Structure:**
```
┌─────────────────────┐        ┌─────────────────────┐
│      Context        │───────▶│     Strategy        │
├─────────────────────┤        ├─────────────────────┤
│ - strategy          │        │ + algorithm()       │
├─────────────────────┤        └──────────┬──────────┘
│ + setStrategy()     │                   │
│ + execute()         │        ┌──────────┴──────────┐
└─────────────────────┘        │                     │
                        ┌──────▼──────┐      ┌──────▼──────┐
                        │ StrategyA   │      │ StrategyB   │
                        └─────────────┘      └─────────────┘
```

**Detection criteria:**
- Context with strategy field (interface type)
- Strategy interface/trait
- Concrete strategy implementations
- `setStrategy()` method in context

### Template Method

**Intent:** Define the skeleton of an algorithm, deferring some steps to subclasses.

**Detection criteria:**
- Abstract class with template method
- Template method calls abstract methods
- Concrete subclasses implement abstract methods

### Visitor

**Intent:** Represent an operation to be performed on elements of an object structure.

## Detection Confidence

The confidence score indicates how closely the detected code matches the pattern template:

| Score | Interpretation |
|-------|----------------|
| 0.90+ | Strong match - all pattern elements present |
| 0.75-0.90 | Good match - most elements present |
| 0.60-0.75 | Partial match - core elements present |
| < 0.60 | Weak match - may be coincidental |

**Factors affecting confidence:**
- Completeness of node mappings
- Match of optional elements
- Naming conventions (e.g., `getInstance` for Singleton)

## Pattern Categories

Access patterns by category:

```rust
use libcpg::patterns::{GofPattern, GofCategory};

// Get category for a pattern
let category = GofPattern::Singleton.category();  // GofCategory::Creational

// Category names
match category {
    GofCategory::Creational => "Object creation patterns",
    GofCategory::Structural => "Object composition patterns",
    GofCategory::Behavioral => "Object interaction patterns",
}
```

## Complete Pattern List

### Creational (5)
| Pattern | Description |
|---------|-------------|
| Abstract Factory | Create families of related objects |
| Builder | Construct complex objects step by step |
| Factory Method | Defer instantiation to subclasses |
| Prototype | Clone existing objects |
| Singleton | Ensure single instance |

### Structural (7)
| Pattern | Description |
|---------|-------------|
| Adapter | Convert interface to expected form |
| Bridge | Separate abstraction from implementation |
| Composite | Tree structures for part-whole |
| Decorator | Add responsibilities dynamically |
| Facade | Unified interface to subsystem |
| Flyweight | Share fine-grained objects |
| Proxy | Surrogate for another object |

### Behavioral (11)
| Pattern | Description |
|---------|-------------|
| Chain of Responsibility | Pass request along chain |
| Command | Encapsulate request as object |
| Interpreter | Grammar representation and interpretation |
| Iterator | Sequential access to collection |
| Mediator | Centralize object interaction |
| Memento | Capture object state externally |
| Observer | One-to-many dependency notification |
| State | Alter behavior based on state |
| Strategy | Interchangeable algorithm family |
| Template Method | Algorithm skeleton with variable steps |
| Visitor | Operations on object structure |

## Next Steps

- [VF2 Matching](vf2-matching.md) - Subgraph isomorphism details
- [Pattern Overview](overview.md) - Back to overview
- [Algorithm Detection](../algorithms/overview.md) - Algorithm patterns

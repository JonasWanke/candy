use super::values::{SolverType, SolverTypeTrait, SolverValue, SolverVariable};
use crate::hir::{self, Trait, TypeParameter};
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::{
    collections::{hash_map::Entry, VecDeque},
    fmt::{self, Display, Formatter},
    iter,
};

fn solver_value_for_types(types: Box<[SolverType]>) -> SolverValue {
    SolverValue {
        type_: "$Placeholder".into(),
        parameters: types,
    }
}

/// The fact that a type implements a trait.
///
/// In the context of the solver, this is displayed as `trait(type)`.
///
/// Examples:
///
/// * `Clone(List<Int>)`: the fact that `List<Int>` implements `Clone`
/// * `Any(?T)`: the fact that `?T` implements `Any`
/// * `Clone(?T)`: the fact that `?T` implements `Clone`
/// * `Iterable(?T, ?L)`: the fact that `?L` implements `Iterable<?T>`
// TODO: separate fields for type and parameters
// TODO: rename goal, rule, etc. to Candy-specific terms
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SolverGoal {
    pub trait_: Box<str>,
    pub parameters: Box<[SolverType]>,
}
impl SolverGoal {
    fn unify(&self, other: &Self) -> Option<FxHashMap<SolverVariable, SolverType>> {
        if self.trait_ != other.trait_ {
            return None;
        };
        SolverType::Value(solver_value_for_types(self.parameters.clone())).unify(
            &SolverType::Value(solver_value_for_types(other.parameters.clone())),
        )
    }
    pub fn substitute_all(&self, substitutions: &FxHashMap<SolverVariable, SolverType>) -> Self {
        Self {
            trait_: self.trait_.clone(),
            parameters: self
                .parameters
                .iter()
                .map(|it| it.substitute_all(substitutions))
                .collect(),
        }
    }
    fn canonicalize(&self) -> Self {
        Self {
            trait_: self.trait_.clone(),
            parameters: solver_value_for_types(self.parameters.clone())
                .canonicalize()
                .parameters,
        }
    }
}
impl TryFrom<TypeParameter> for SolverGoal {
    type Error = ();

    fn try_from(value: TypeParameter) -> Result<Self, Self::Error> {
        let Some(hir::Ok(Trait {
            name,
            type_arguments,
        })) = &value.upper_bound
        else {
            return Err(());
        };

        // FIXME
        Ok(Self {
            trait_: name.clone(),
            parameters: type_arguments
                .iter()
                .map(|it| SolverType::try_from(it.clone()).unwrap())
                .chain(iter::once(SolverVariable::new(value.type_()).into()))
                .collect(),
        })
    }
}
impl Display for SolverGoal {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}({})", self.trait_, self.parameters.iter().join(", "))
    }
}

/// An inference rule, usually generated based on impls.
///
/// In the context of the solver, these are displayed as `goal <- subgoals`.
///
/// Examples:
///
/// * `Clone(Tuple<?0, ?1>) <- Clone(?0), Clone(?1)`: the fact that `Tuple<?0, ?1>` implements
///   `Clone` if both `?0` and `?1` implement `Clone`
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SolverRule {
    // originalImpl: HirTrait | HirType | HirNamedType | HirImpl, // FIXME
    /// * `HirTrait` or `HirType`: originates from the `Any` or an error type, or `T(?T)`
    /// * `HirNamedType`: originates from that trait upper bound
    /// * `HirImpl`: originates from that impl
    // TODO(never): rename this to `source`
    pub goal: SolverGoal,

    pub subgoals: Box<[SolverGoal]>,
}
impl Display for SolverRule {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{} <- {}",
            self.goal,
            if self.subgoals.is_empty() {
                "<nothing>".to_string()
            } else {
                self.subgoals.iter().join(", ")
            }
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Environment {
    pub rules: Vec<SolverRule>,
}
impl Environment {
    /// Checks if the goals are true if the conditions are met.
    ///
    /// For example, to check if `Iterable[Equals]` implements `Equals`, you might try to achieve
    /// the goal `Equals(?0)` under the conditions `Iterable(?T, ?0), Equals(?T)`.
    // TODO(never, marcelgarus): Somehow support solving logic that has multiple results. Currently, rules
    // are of the form `consequence <- conditions` (with only one consequence), but we'd need multiple
    // consequences to express something like "Implement `Iterable[Equals]`". That's because there's
    // no single `SolverGoal` that the trait `Iterable[Equals]` can be lowered to – `Iterable(Equals)`
    // is not valid, because `Equals` is a trait, not a type. Essentially, we'd need something like
    // `Iterable(?0, ?1), Equals(?1)`, e.g. "Are there ?0 and ?1 so that these goals are true?"
    // I believe that Chalk, Rust's to-be type solver, manages this using something called
    // [Opaque Types](https://rust-lang.github.io/chalk/book/clauses/opaque_types.html), but I'm not
    // entirely sure. In Rust, this would correspond to an `impl Iterable<Box<dyn Equals>> for Foo`.
    // As we rely even more than Rust on dynamic dispatch and being able to implement traits like
    // `List[Equals]`, we'll definitely have to incorporate something like that into our solver
    // someday.
    pub fn solve(&self, goal: &SolverGoal, conditions: &[SolverGoal]) -> SolverSolution {
        // Create virtual types for the types in the rule and implement the right traits for them. In
        // the example above, we'd create virtual `$Virtual0` and `$Virtual1` types and implement the
        // given traits for them, e.g. by adding the rules `Iterable($Virtual1, $Virtual0) <- ∅` and
        // `Equals($Virtual0) <- ∅`. Then, we can ask a `Solver` whether `Equals($Virtual0)` holds.
        let mut canonical_mapping = FxHashMap::default();
        // Canonicalized rules like `Iterable(?1, ?0)` and `Equals(?0)` (without `?T`).
        let canonical_conditions = conditions
            .iter()
            .map(|it| SolverGoal {
                trait_: it.trait_.clone(),
                parameters: solver_value_for_types(it.parameters.clone())
                    .canonicalize_internal(&mut canonical_mapping)
                    .parameters,
            })
            .collect_vec();
        // Create a mapping from canonical variables like `?0` to virtual types like `$Virtual0`.
        let canonical_to_virtual = canonical_mapping
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    SolverType::Value(SolverValue {
                        type_: format!("$Virtual${}", value.type_.name).into_boxed_str(),
                        parameters: Box::default(),
                    }),
                )
            })
            .collect();
        let virtual_rules = canonical_conditions
            .iter()
            .map(|it| it.substitute_all(&canonical_to_virtual))
            .map(|goal| SolverRule {
                goal,
                subgoals: Box::default(),
            });

        let new_rules = self.rules.iter().cloned().chain(virtual_rules).collect();
        Solver::new(new_rules).solve(&goal.substitute_all(&canonical_to_virtual))
    }
}
impl Display for Environment {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Environment(")?;
        for rule in &self.rules {
            write!(f, "\n  {rule}")?;
        }
        write!(f, "\n)")
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SolverSolution {
    /// There is exactly one type that can fulfill the goal.
    Unique(SolverSolutionUnique),

    /// There may be multiple types that can fulfill the goal.
    Ambiguous,

    /// It's impossible to fulfill the goal.
    Impossible,
}
impl Display for SolverSolution {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Unique(solution) => write!(f, "Unique({solution})"),
            Self::Ambiguous => write!(f, "Ambiguous"),
            Self::Impossible => write!(f, "Impossible"),
        }
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SolverSolutionUnique {
    pub refined_goal: SolverGoal,
    pub used_rule: SolverRule,
}
impl Display for SolverSolutionUnique {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "SolverSolutionUnique(refined goal: {}, used rule: {})",
            self.refined_goal, self.used_rule,
        )
    }
}

#[derive(Debug)]
struct Solver {
    rules: Vec<SolverRule>,

    /// Map that allows us to detect cycles and resolve them.
    ///
    /// It's also good for performance. For example, when checking whether
    /// `Clone(Tuple<Int, Int>)`, it doesn't make sense to prove `Clone(Int)`
    /// twice.
    cache: FxHashMap<SolverGoal, SolverTreeState>,
}
impl Solver {
    fn new(rules: Vec<SolverRule>) -> Self {
        Self {
            rules,
            cache: FxHashMap::default(),
        }
    }

    fn solve(&mut self, goal: &SolverGoal) -> SolverSolution {
        let goal = goal.canonicalize();
        match self.cache.entry(goal.clone()) {
            Entry::Occupied(mut entry) => match entry.get() {
                SolverTreeState::BeingSolved => {
                    entry.insert(SolverTreeState::Solved(SolverSolution::Ambiguous));
                    return SolverSolution::Ambiguous;
                }
                SolverTreeState::Solved(solution) => return solution.clone(),
            },
            Entry::Vacant(entry) => {
                entry.insert(SolverTreeState::BeingSolved);
            }
        }
        let solution = SolverTree { goal: goal.clone() }.solve(self);
        self.cache
            .insert(goal, SolverTreeState::Solved(solution.clone()));
        solution
    }
}

#[derive(Debug)]
enum SolverTreeState {
    BeingSolved,
    Solved(SolverSolution),
}
#[derive(Debug)]
struct SolverTree {
    goal: SolverGoal,
}
impl SolverTree {
    fn solve(&self, context: &mut Solver) -> SolverSolution {
        let mut strands = context
            .rules
            .iter()
            .filter(|it| it.goal.unify(&self.goal).is_some())
            .map(|it| {
                let result = it.goal.unify(&self.goal).unwrap();
                let subgoals = it
                    .subgoals
                    .iter()
                    .map(|it| it.substitute_all(&result))
                    .collect();
                SolverStrand {
                    used_rule: it.clone(),
                    goal: it.goal.substitute_all(&result),
                    solution_space: result,
                    subgoals,
                }
            })
            .collect_vec();
        let mut strands_and_solutions = strands
            .iter_mut()
            .map(|it| {
                let solution = it.solve(context);
                (it, solution)
            })
            .filter(|(_, solution)| solution != &SolverSolution::Impossible)
            .collect_vec();
        if strands_and_solutions.len() > 1 {
            return SolverSolution::Ambiguous;
        }
        if strands_and_solutions.is_empty() {
            return SolverSolution::Impossible;
        }
        let (strand, solution) = strands_and_solutions.pop().unwrap();
        if solution == SolverSolution::Ambiguous {
            return SolverSolution::Ambiguous;
        }
        let SolverSolution::Unique(solution) = solution else {
            panic!()
        };
        let substitutions = strand.goal.unify(&solution.refined_goal).unwrap();
        let refined_goal = self.goal.substitute_all(&substitutions);
        SolverSolution::Unique(SolverSolutionUnique {
            refined_goal,
            used_rule: solution.used_rule,
        })
    }
}

/// A thread exploring possible solutions.
#[derive(Debug)]
struct SolverStrand {
    used_rule: SolverRule,
    goal: SolverGoal,
    solution_space: FxHashMap<SolverVariable, SolverType>,
    subgoals: VecDeque<SolverGoal>,
}
impl SolverStrand {
    fn solve(&mut self, context: &mut Solver) -> SolverSolution {
        while let Some(subgoal) = self.subgoals.pop_front() {
            let subgoal = subgoal.canonicalize();
            let solution = context.solve(&subgoal);
            match solution {
                SolverSolution::Impossible => return SolverSolution::Impossible,
                SolverSolution::Unique(solution) => {
                    // Integrate the solution into the solution space.
                    let substitutions = subgoal.unify(&solution.refined_goal).unwrap();
                    for (variable, type_) in substitutions {
                        if let Some(substitution) = self.solution_space.get(&variable) {
                            let Some(new_substitutions) = substitution.unify(&type_) else {
                                return SolverSolution::Impossible;
                            };
                            self.solution_space.values_mut().for_each(|it| {
                                it.substitute_all(&new_substitutions);
                            });
                        } else {
                            self.solution_space.insert(variable, type_);
                        }
                    }
                }
                SolverSolution::Ambiguous => {}
            }
        }
        SolverSolution::Unique(SolverSolutionUnique {
            refined_goal: self.goal.substitute_all(&self.solution_space),
            used_rule: self.used_rule.clone(),
        })
    }
}

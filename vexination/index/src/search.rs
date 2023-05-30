use sikula::mir;
use sikula::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vulnerabilities<'a> {
    Id(Primary<'a>),
    Cve(Primary<'a>),
    Title(Primary<'a>),
    Description(Primary<'a>),
    Status(Primary<'a>),
    Severity(Primary<'a>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum VulnerabilitiesSortable {}

impl FromQualifier for VulnerabilitiesSortable {
    type Err = ();

    fn from_qualifier(qualifier: &mir::Qualifier) -> Result<Self, Self::Err> {
        Ok(match qualifier.as_slice() {
            _ => return Err(()),
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum VulnerabilitiesScope {
    Id,
    Cve,
    Title,
    Description,
    Status,
    Severity,
}

impl FromQualifier for VulnerabilitiesScope {
    type Err = ();

    fn from_qualifier(qualifier: &mir::Qualifier) -> Result<Self, Self::Err> {
        Ok(match qualifier.as_slice() {
            ["id"] => Self::Id,
            ["cve"] => Self::Cve,
            ["title"] => Self::Title,
            ["description"] => Self::Description,
            ["status"] => Self::Status,
            ["severity"] => Self::Severity,
            _ => return Err(()),
        })
    }
}

impl<'a> Resource<'a> for Vulnerabilities<'a> {
    type Parsed = Vulnerabilities<'a>;
    type Sortable = VulnerabilitiesSortable;
    type Scope = VulnerabilitiesScope;

    fn default_scopes() -> Vec<Self::Scope> {
        vec![
            Self::Scope::Id,
            Self::Scope::Cve,
            Self::Scope::Title,
            Self::Scope::Description,
            Self::Scope::Severity,
            Self::Scope::Status,
        ]
    }

    fn parse_query(q: &'a str) -> Result<Query<Self>, Error> {
        use chumsky::Parser;

        let query = mir::Query::parse(
            parser()
                .parse(q)
                .into_result()
                .map_err(|s| Error::Parser(s.into_iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n")))?,
        )?;

        let scopes = if query.scope.is_empty() {
            Self::default_scopes()
        } else {
            let mut scopes = Vec::with_capacity(query.scope.len());
            for qualifier in query.scope {
                scopes.push(
                    Self::Scope::from_qualifier(&qualifier).map_err(|()| Error::UnknownScopeQualifier(qualifier))?,
                );
            }
            scopes
        };

        let mut terms = vec![];
        for term in query.terms {
            let invert = term.invert;
            let mut term = match term.expression {
                mir::Expression::Predicate => match term.qualifier.as_slice() {
                    ["final"] => Term::Match(Self::Status(Primary::Equal("final"))),
                    ["critical"] => Term::Match(Self::Severity(Primary::Equal("critical"))),
                    ["high"] => Term::Match(Self::Severity(Primary::Equal("high"))),
                    ["medium"] => Term::Match(Self::Severity(Primary::Equal("medium"))),
                    ["low"] => Term::Match(Self::Severity(Primary::Equal("low"))),
                    _ => return Err(Error::UnknownPredicate(term.qualifier)),
                },
                mir::Expression::Simple(expression) => match term.qualifier.as_slice() {
                    [] => {
                        // primary
                        let mut terms = vec![];
                        for scope in &scopes {
                            let expression = match scope {
                                Self::Scope::Id => Term::Match(Self::Id(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                                Self::Scope::Cve => Term::Match(Self::Cve(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                                Self::Scope::Title => Term::Match(Self::Title(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                                Self::Scope::Description => Term::Match(Self::Description(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                                Self::Scope::Severity => Term::Match(Self::Severity(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                                Self::Scope::Status => Term::Match(Self::Status(
                                    expression.into_expression(QualifierContext::Primary, mir::Qualifier::empty())?,
                                )),
                            };
                            terms.push(expression);
                        }
                        Term::Or(terms)
                    }
                    ["id", n @ ..] => Term::Match(Self::Id(
                        expression.into_expression(QualifierContext::Qualifier, n.into())?,
                    )),
                    _ => return Err(Error::UnknownQualifier(term.qualifier)),
                },
            };

            if invert {
                term = Term::Not(Box::new(term));
            }

            terms.push(term);
        }

        let mut sorting = vec![];
        for sort in query.sorting {
            sorting.push(Sort {
                qualifier: Self::Sortable::from_qualifier(&sort.qualifier)
                    .map_err(|()| Error::UnknownSortQualifier(sort.qualifier))?,
                direction: sort.direction,
            })
        }

        Ok(Query {
            term: Term::And(terms).compact(),
            sorting,
        })
    }
}

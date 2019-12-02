use crate::model::*;
use crate::vocab::*;

use curie::PrefixMapping;

use failure::Error;

use sophia::term::IriData;
use sophia::term::Term;

use std::io::BufRead;
use std::rc::Rc;

enum AcceptState {
    // Accept and consume the triple
    Accept,

    // Do not accept the triple and return it
    Return([Term<Rc<str>>; 3]),

    // Return triples that were previously accepted
    BackTrack(Vec<[Term<Rc<str>>; 3]>),
}

enum CompleteState {
    // The acceptor does not have all of the information that it needs
    // because it has not recieved enough triples.
    NotComplete,

    // The acceptor has all the information that it needs to complete,
    // but can still accept more. The acceptor may require further
    // information from the ontology, for instance the type of IRIs in
    // accepted triples, to actually complete.
    CanComplete,

    // The acceptor has all the information that it needs to complete,
    // and will not accept further triples. The acceptor may require
    // further information from the ontology, for instance the type of
    // IRIs in accepted triples, to actually complete.
    Complete,
}

trait Acceptor<O>: std::fmt::Debug {
    // Accept a triple.
    fn accept(&mut self, b: &Build, triple: [Term<Rc<str>>; 3]) -> AcceptState;

    // Indicate the completion state of the acceptor.
    fn can_complete(&mut self) -> CompleteState;

    // Return an ontology entity based on the triples that have been
    // consumed and the data in the ontology. Return an Error if the
    // acceptor cannot return yet because there is information missing
    // from the ontology (such as a declaration).

    // This method should not be called till can_complete would return
    // either Complete or CanComplete if called.

    // TODO: Do I need more than one Error here? I know for sure that
    // I will need to return "not got enough information from the
    // ontology"; I guess I should be able to return a "not in the
    // right state" error also. Anything else?
    fn complete(self, b: &Build, o: &Ontology) -> Result<O, Error>;
}

#[derive(Debug, Default)]
struct OntologyAcceptor {
    incomplete: Vec<AnnotatedAxiomAcceptor>,
    complete_acceptors: Vec<AnnotatedAxiomAcceptor>,

    // Does this make any sense -- we are replicating the Ontology
    // data structure here? And our data structures are
    // complicated. Why do we not build these up as we go?

    // Because now we have to convert backwards and forwards between
    // the sophia data structures -- think it was better before. It
    // will probably help anyway when we come to structures were we
    // cannot work it out early
    iri: Option<IriData<Rc<str>>>,
    viri: Option<IriData<Rc<str>>>
}

impl Acceptor<Ontology> for OntologyAcceptor {
    fn accept(&mut self, b:&Build, triple: [Term<Rc<str>>; 3]) -> AcceptState {
        match &triple {
            [Term::Iri(s), Term::Iri(p), Term::Iri(ob)]
                if p == &"http://www.w3.org/1999/02/22-rdf-syntax-ns#type" &&
                ob == &OWL::Ontology.iri_str() =>
            {
                self.iri = Some(s.clone());
                AcceptState::Accept
            }
            [Term::Iri(s), Term::Iri(p), Term::Iri(ob)]
                if self.iri.as_ref() == Some(s) &&
                p == &OWL::VersionIRI.iri_str() =>
            {
                self.viri = Some(ob.clone());
                AcceptState::Accept
            }
            _ => {
                // Pass on to incomplete acceptors, till one of them
                // accepts, then pass onto new acceptors and see if
                // one of them accepts. Collect and collate any "backtracks",
                // return one of these.
                let mut d = AnnotatedAxiomAcceptor::default();
                match d.accept(b, triple) {
                    AcceptState::Accept => {
                        // this only works because declaration
                        // accepts are one long
                        self.complete_acceptors.push(d);
                        AcceptState::Accept
                    },
                    AcceptState::Return(t) => {
                        AcceptState::Return(t)
                    },
                    AcceptState::BackTrack(v) => {
                        AcceptState::BackTrack(v)
                    },
                }
            }
        }
    }

    fn can_complete(&mut self) -> CompleteState {
        unimplemented!()
    }

    fn complete(self, b: &Build, o:&Ontology) -> Result<Ontology, Error> {
        // Iterate over all the complete Acceptor, run complete on
        // them, and insert this
        let mut o = Ontology::default();
        o.id.iri = self.iri.map(|i| b.iri(i.to_string()));
        o.id.viri = self.viri.map(|i| b.iri(i.to_string()));

        // TODO: deal with incomplete acceptors
        for ac in self.complete_acceptors{
           o.insert(ac.complete(b, &o)?);
        }
        return Ok(o);
    }
}

#[derive(Debug, Default)]
struct AnnotatedAxiomAcceptor {
    iri: Option<IriData<Rc<str>>>,
}

impl Acceptor<AnnotatedAxiom> for AnnotatedAxiomAcceptor {
    fn accept(&mut self, b:&Build, triple: [Term<Rc<str>>; 3]) -> AcceptState {
        match &triple {
            [Term::Iri(s), Term::Iri(p), Term::Iri(ob)]
                if p == &"http://www.w3.org/1999/02/22-rdf-syntax-ns#type" =>
            {
                self.iri = Some(s.clone());
                AcceptState::Accept
            }
            _=> {
                dbg!(triple);
                unimplemented!()
            }
        }
    }

    fn can_complete(&mut self) -> CompleteState {
        unimplemented!()
    }

    fn complete(self, b: &Build, o:&Ontology) -> Result<AnnotatedAxiom, Error> {
        // Iterate over all the complete Acceptor, run complete on
        // them, and insert this
        let n:NamedEntity = b.class(self.iri.unwrap().to_string()).into();
        Ok(declaration(n).into())
    }
}

fn read_then_complete(
    triple_iter: impl Iterator<Item = Result<[Term<Rc<str>>; 3], sophia::error::Error>>,
    b: &Build,
    mut acceptor: OntologyAcceptor,
) -> Result<Ontology, Error> {
    for t in triple_iter {
        let t = t.unwrap();
        acceptor.accept(b, t);
    }

    acceptor.complete(b, &Ontology::default())
}

pub fn read_with_build<R: BufRead>(
    bufread: &mut R,
    build: &Build,
) -> Result<(Ontology, PrefixMapping), Error> {
    let parser = sophia::parser::xml::Config::default();
    let triple_iter = parser.parse_bufread(bufread);

    return read_then_complete(triple_iter, build, OntologyAcceptor::default()).
        map (|o| return (o, PrefixMapping::default()));
}

pub fn read<R: BufRead>(bufread: &mut R) -> Result<(Ontology, PrefixMapping), Error> {
    let b = Build::new();
    read_with_build(bufread, &b)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    fn read_ok<R: BufRead>(bufread: &mut R) -> (Ontology, PrefixMapping) {
        let r = read(bufread);

        assert!(r.is_ok(), "Expected ontology, get failure: {:?}", r.err());
        r.unwrap()
    }

    fn compare(test: &str) {
        let dir_path_buf = PathBuf::from(file!());
        let dir = dir_path_buf.parent().unwrap().to_string_lossy();

        compare_str(
            &slurp::read_all_to_string(format!("{}/../../ont/owl-rdf/{}.owl", dir, test)).unwrap(),
            &slurp::read_all_to_string(format!("{}/../../ont/owl-xml/{}.owx", dir, test)).unwrap(),
        );
    }

    fn compare_str(rdfread: &str, xmlread: &str) {
        let (rdfont, _rdfmapping) = read_ok(&mut rdfread.as_bytes());
        let (xmlont, _xmlmapping) = crate::io::reader::test::read_ok(&mut xmlread.as_bytes());

        assert_eq!(rdfont, xmlont);

        //let rdfmapping: &HashMap<&String, &String> = &rdfmapping.mappings().collect();
        //let xmlmapping: &HashMap<&String, &String> = &xmlmapping.mappings().collect();

        //assert_eq!(rdfmapping, xmlmapping);
    }

    #[test]
    fn one_class() {
        compare("one-class");
    }

    //#[test]
    //fn declaration_with_annotation() {
    //    compare("declaration-with-annotation");
    //}

    // #[test]
    // fn class_with_two_annotations() {
    //     compare("class_with_two_annotations");
    // }

    #[test]
    fn one_ont() {
        compare("one-ont");
    }

    // #[test]
    // fn round_one_ont_prefix() {
    //     let (_ont_orig, prefix_orig, _ont_round, prefix_round) =
    //         roundtrip(include_str!("../ont/owl-xml/one-ont.owx"));

    //     let prefix_orig_map: HashMap<&String, &String> = prefix_orig.mappings().collect();

    //     let prefix_round_map: HashMap<&String, &String> = prefix_round.mappings().collect();

    //     assert_eq!(prefix_orig_map, prefix_round_map);
    //}

    // #[test]
    // fn one_subclass() {
    //     compare("one-subclass");
    // }

    // #[test]
    // fn subclass_with_annotation() {
    //     compare("annotation-on-subclass");
    // }

    // #[test]
    // fn one_oproperty() {
    //     compare("one-oproperty");
    // }

    // #[test]
    // fn one_some() {
    //     compare("one-some");
    // }

    // #[test]
    // fn one_only() {
    //     compare("one-only");
    // }

    // #[test]
    // fn one_and() {
    //     compare("one-and");
    // }

    // #[test]
    // fn one_or() {
    //     compare("one-or");
    // }

    // #[test]
    // fn one_not() {
    //     compare("one-not");
    // }

    // #[test]
    // fn one_annotation_property() {
    //     compare("one-annotation-property");
    // }

    // #[test]
    // fn one_annotation() {
    //     compare("one-annotation");
    // }

    // #[test]
    // fn annotation_domain() {
    //     compare("annotation-domain");
    // }

    // #[test]
    // fn annotation_range() {
    //     compare("annotation-range");
    // }

    // #[test]
    // fn one_label() {
    //     compare("one-label");
    // }

    // #[test]
    // fn one_comment() {
    //     // This is currently failing because the XML parser gives the
    //     // comment a language and a datatype ("PlainLiteral") while
    //     // the RDF one gives it just the language, as literals can't
    //     // be both. Which is correct?
    //     compare("one-comment");
    // }

    // #[test]
    // fn one_ontology_annotation() {
    //     compare("one-ontology-annotation");
    // }

    // #[test]
    // fn one_equivalent_class() {
    //     compare("one-equivalent");
    // }

    // #[test]
    // fn one_disjoint_class() {
    //     compare("one-disjoint");
    // }

    // #[test]
    // fn disjoint_union() {
    //     compare("disjoint-union");
    // }

    // #[test]
    // fn one_sub_property() {
    //     compare("one-suboproperty");
    // }

    // #[test]
    // fn one_inverse() {
    //     compare("inverse-properties");
    // }

    // #[test]
    // fn one_transitive() {
    //     compare("transitive-properties");
    // }

    // #[test]
    // fn one_annotated_transitive() {
    //     compare("annotation-on-transitive");
    // }

    // #[test]
    // fn one_subproperty_chain() {
    //     compare("subproperty-chain");
    // }

    // #[test]
    // fn one_subproperty_chain_with_inverse() {
    //     compare("subproperty-chain-with-inverse");
    // }

    // #[test]
    // fn annotation_on_annotation() {
    //     compare("annotation-with-annotation");
    // }

    // #[test]
    // fn sub_annotation() {
    //     compare("sub-annotation");
    // }

    // #[test]
    // fn data_property() {
    //     compare("data-property");
    // }

    // #[test]
    // fn literal_escaped() {
    //     compare("literal-escaped");
    // }

    // #[test]
    // fn named_individual() {
    //     compare("named-individual");
    // }

    // #[test]
    // fn import() {
    //     compare("import");
    // }

    // #[test]
    // fn datatype() {
    //    compare("datatype");
    //}

    // #[test]
    // fn object_has_value() {
    //     compare("object-has-value");
    // }

    // #[test]
    // fn object_one_of() {
    //     compare("object-one-of");
    // }

    // #[test]
    // fn inverse() {
    //     compare("some-inverse");
    // }

    // #[test]
    // fn object_unqualified_cardinality() {
    //     compare("object-unqualified-max-cardinality");
    // }

    // #[test]
    // fn object_min_cardinality() {
    //     compare("object-min-cardinality");
    // }

    // #[test]
    // fn object_max_cardinality() {
    //     compare("object-max-cardinality");
    // }

    // #[test]
    // fn object_exact_cardinality() {
    //     compare("object-exact-cardinality");
    // }

    // #[test]
    // fn datatype_alias() {
    //     compare("datatype-alias");
    // }

    // #[test]
    // fn datatype_intersection() {
    //     compare("datatype-intersection");
    // }

    // #[test]
    // fn datatype_union() {
    //     compare("datatype-union");
    // }

    // #[test]
    // fn datatype_complement() {
    //     compare("datatype-complement");
    // }

    // #[test]
    // fn datatype_oneof() {
    //     compare("datatype-oneof");
    // }

    // #[test]
    // fn datatype_some() {
    //    compare("data-some");
    // }

    // #[test]
    // fn facet_restriction() {
    //     compare("facet-restriction");
    // }

    // #[test]
    // fn data_only() {
    //     compare("data-only");
    // }

    // #[test]
    // fn data_exact_cardinality() {
    //     compare("data-exact-cardinality");
    // }

    // #[test]
    // fn data_has_value() {
    //     compare("data-has-value");
    // }

    // #[test]
    // fn data_max_cardinality() {
    //     compare("data-max-cardinality");
    // }

    // #[test]
    // fn data_min_cardinality() {
    //     compare("data-min-cardinality");
    // }

    // #[test]
    // fn class_assertion() {
    //     compare("class-assertion");
    // }

    // #[test]
    // fn data_property_assertion() {
    //     compare("data-property-assertion");
    // }

    // #[test]
    // fn same_individual() {
    //     compare("same-individual");
    // }

    // #[test]
    // fn different_individuals() {
    //     compare("different-individual");
    // }

    // #[test]
    // fn negative_data_property_assertion() {
    //     compare("negative-data-property-assertion");
    // }

    // #[test]
    // fn negative_object_property_assertion() {
    //     compare("negative-object-property-assertion");
    // }

    // #[test]
    // fn object_property_assertion() {
    //     compare("object-property-assertion");
    // }

    // #[test]
    // fn data_has_key() {
    //     compare("data-has-key");
    // }

    // #[test]
    // fn data_property_disjoint() {
    //     compare("data-property-disjoint");
    // }

    // #[test]
    // fn data_property_domain() {
    //     compare("data-property-domain");
    // }

    // #[test]
    // fn data_property_equivalent() {
    //     compare("data-property-equivalent");
    // }

    // #[test]
    // fn data_property_functional() {
    //     compare("data-property-functional");
    // }

    // #[test]
    // fn data_property_range() {
    //     compare("data-property-range");
    // }

    // #[test]
    // fn data_property_sub() {
    //     compare("data-property-sub");
    // }

    // #[test]
    // fn disjoint_object_properties() {
    //     compare("disjoint-object-properties");
    // }

    // #[test]
    // fn equivalent_object_properties() {
    //     compare("equivalent_object_properties");
    // }

    // #[test]
    // fn object_has_key() {
    //     compare("object-has-key");
    // }

    // #[test]
    // fn object_property_asymmetric() {
    //     compare("object-property-asymmetric");
    // }

    // #[test]
    // fn object_property_domain() {
    //     compare("object-property-domain");
    // }

    // #[test]
    // fn object_property_functional() {
    //     compare("object-property-functional");
    // }

    // #[test]
    // fn object_property_inverse_functional() {
    //     compare("object-property-inverse-functional");
    // }

    // #[test]
    // fn object_property_irreflexive() {
    //     compare("object-property-irreflexive");
    // }

    // #[test]
    // fn object_property_range() {
    //     compare("object-property-range");
    // }

    // #[test]
    // fn object_property_reflexive() {
    //     compare("object-property-reflexive");
    // }

    // #[test]
    // fn object_property_symmetric() {
    //     compare("object-property-symmetric");
    // }

    // #[test]
    // fn family() {
    //     compare("family");
    // }
}
use plotter_chart::{Channel, ChartSpec, Coordinate, Guide, Mark, codes};

use crate::support::has_code;

#[test]
fn a_new_spec_keeps_the_mark_and_both_default_axes() {
    let spec = ChartSpec::new(Mark::Line);

    assert_eq!(spec.mark, Mark::Line);
    assert_eq!(spec.coordinate, Coordinate::Cartesian);
    assert_eq!(spec.guides, vec![Guide::axis(Channel::X), Guide::axis(Channel::Y)]);
    assert!(has_code(&spec.validate(), codes::INVALID_SPEC), "a spec without bindings must be rejected");
}

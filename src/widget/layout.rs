use cassowary::{Solver, Variable, Constraint};
use cassowary::WeightedRelation::*;
use cassowary::strength::*;

use util::{Point, Rectangle, Dimensions, Scalar};

#[derive(Copy, Clone)]
pub enum Orientation {
    Horizontal,
    Vertical,
}
pub struct LinearLayout {
    pub orientation: Orientation,
    pub end: Variable,
}
impl LinearLayout {
    pub fn new(orientation: Orientation, parent: &WidgetLayout) -> Self {
        LinearLayout {
            orientation: orientation,
            end: LinearLayout::beginning(orientation, parent),
        }
    }
    pub fn beginning(orientation: Orientation, layout: &WidgetLayout) -> Variable {
        match orientation {
            Orientation::Horizontal => layout.left,
            Orientation::Vertical => layout.top,
        }
    }
    pub fn ending(orientation: Orientation, layout: &WidgetLayout) -> Variable {
        match orientation {
            Orientation::Horizontal => layout.right,
            Orientation::Vertical => layout.bottom,
        }
    }
    pub fn add_widget(&mut self, widget_layout: &mut WidgetLayout) {
        let constraint = LinearLayout::beginning(self.orientation, &widget_layout) |
                         GE(REQUIRED) | self.end;
        self.end = LinearLayout::ending(self.orientation, &widget_layout);
        widget_layout.add_constraint(constraint);
    }
}

pub struct WidgetLayout {
    pub left: Variable,
    pub top: Variable,
    pub right: Variable,
    pub bottom: Variable,
    pub scrollable: bool,
    pub constraints: Vec<Constraint>,
}
impl WidgetLayout {
    pub fn new() -> Self {
        WidgetLayout {
            left: Variable::new(),
            top: Variable::new(),
            right: Variable::new(),
            bottom: Variable::new(),
            scrollable: false,
            constraints: Vec::new(),
        }
    }
    pub fn bounds(&self, solver: &mut Solver) -> Rectangle {
        Rectangle {
            left: solver.get_value(self.left),
            top: solver.get_value(self.top),
            width: solver.get_value(self.right) - solver.get_value(self.left),
            height: solver.get_value(self.bottom) - solver.get_value(self.top),
        }
    }
    pub fn get_dims(&self, solver: &mut Solver) -> Dimensions {
        let bounds = self.bounds(solver);
        Dimensions {
            width: bounds.width,
            height: bounds.height,
        }
    }
    pub fn update_solver(&self, solver: &mut Solver) {
        let constraints = self.constraints.clone();
        for constraint in constraints {
            if !solver.has_constraint(&constraint) {
                solver.add_constraint(constraint.clone()).unwrap();
            }
        }
    }
    pub fn add_child(&self, child_layout: &mut WidgetLayout) {
        if self.scrollable {
            child_layout.scroll_inside(self);
        } else {
            child_layout.bound_by(self);
        }
    }
    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }
    pub fn add_constraints(&mut self, constraints: &[Constraint]) {
        self.constraints.extend_from_slice(constraints);
    }
    pub fn match_layout(&mut self, layout: &WidgetLayout) {
        self.match_width(layout);
        self.match_height(layout);
    }
    pub fn match_width(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.left | EQ(REQUIRED) | layout.left);
        self.constraints.push(self.right | EQ(REQUIRED) | layout.right);
    }
    pub fn match_height(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.top | EQ(REQUIRED) | layout.top);
        self.constraints.push(self.bottom | EQ(REQUIRED) | layout.bottom);
    }
    pub fn width(&mut self, width: Scalar) {
        self.constraints.push(self.right - self.left | EQ(REQUIRED) | width)
    }
    pub fn height(&mut self, height: Scalar) {
        self.constraints.push(self.bottom - self.top | EQ(REQUIRED) | height)
    }
    pub fn dimensions(&mut self, dimensions: Dimensions) {
        self.width(dimensions.width);
        self.height(dimensions.height);
    }
    pub fn width_strength(&mut self, width: Scalar, strength: f64) {
        self.constraints.push(self.right - self.left | EQ(strength) | width)
    }
    pub fn height_strength(&mut self, height: Scalar, strength: f64) {
        self.constraints.push(self.bottom - self.top | EQ(strength) | height)
    }
    pub fn top_left(&mut self, point: Point) {
        self.constraints.push(self.left | EQ(REQUIRED) | point.x);
        self.constraints.push(self.top | EQ(REQUIRED) | point.y);
    }
    pub fn pad(&mut self, distance: Scalar, outer_layout: &WidgetLayout) {
        self.constraints.push(self.left - outer_layout.left | GE(REQUIRED) | distance);
        self.constraints.push(self.top - outer_layout.top | GE(REQUIRED) | distance);
        self.constraints.push(outer_layout.right - self.right | GE(REQUIRED) | distance);
        self.constraints.push(outer_layout.bottom - self.bottom | GE(REQUIRED) | distance);
    }
    pub fn center(&mut self, layout: &WidgetLayout) {
        self.center_horizontal(layout);
        self.center_vertical(layout);
    }
    pub fn center_horizontal(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.left - layout.left | EQ(REQUIRED) | layout.right - self.right);
    }
    pub fn center_vertical(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.top - layout.top | EQ(REQUIRED) | layout.bottom - self.bottom);
    }
    pub fn align_top(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.top | EQ(REQUIRED) | layout.top);
    }
    pub fn align_bottom(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.bottom | EQ(REQUIRED) | layout.bottom);
    }
    pub fn align_left(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.left | EQ(REQUIRED) | layout.left);
    }
    pub fn align_right(&mut self, layout: &WidgetLayout) {
        self.constraints.push(self.right | EQ(REQUIRED) | layout.right);
    }
    pub fn bound_by(&mut self, layout: &WidgetLayout) {
        let constraints = [self.left | GE(REQUIRED) | layout.left,
                           self.top | GE(REQUIRED) | layout.top,
                           self.right | LE(REQUIRED) | layout.right,
                           self.bottom | LE(REQUIRED) | layout.bottom];
        self.add_constraints(&constraints);
    }
    pub fn scroll_inside(&mut self, layout: &WidgetLayout) {
        let constraints = [self.left | LE(REQUIRED) | layout.left,
                           self.top | LE(REQUIRED) | layout.top,
                           // STRONG not REQUIRED because not satisfiable if layout is smaller than it's parent
                           self.right | GE(STRONG) | layout.right,
                           self.bottom | GE(STRONG) | layout.bottom];
        self.add_constraints(&constraints);
    }
}

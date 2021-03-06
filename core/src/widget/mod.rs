//! Types used to create and manage widgets.
//!
//! Limn UIs exist as a tree of `Widget`s, each of which consists of a bounding rectangle,
//! a list of references to it's children, a list of `EventHandler`s that receive and send events,
//! and optionally a draw state struct that implements `Draw`.
//!
//! The root widget is just an ordinary widget that happens to be stored by the `Ui` so it can be drawn, and that
//! has the size of the window as its bounding rectangle.
//!
//! Creating a user interface consists of constructing a widget tree, then passing the `Widget` root of
//! that tree to a limn `App`, which will attach it to the `Ui` root widget for you and begin the event loop.

pub mod property;
pub mod draw;
pub mod filter;

use std::any::{TypeId, Any};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::cell::{RefCell, Ref, RefMut};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::fmt;
use std::fmt::Debug;

use render::RenderBuilder;
use event::{self, EventHandler, EventArgs, EventHandlerWrapper};
use layout::{Layout, LayoutVars, LayoutRef, LayoutUpdated, VarType};
use ui::Ui;
use resources::{resources, WidgetId};
use geometry::{Point, Rect};
use render;
use color::Color;
use event::Target;
use layout::UpdateLayout;
use style::*;

use widget::filter::Filter;

use self::property::{PropSet, Property};
use self::draw::*;

#[derive(Clone, Copy)]
pub struct StateUpdated;

#[derive(Debug, Copy, Clone)]
pub struct StyleUpdated;

#[derive(Clone)]
pub struct Widget(Rc<RefCell<WidgetInner>>);

impl Widget {
    /// Creates a new, named `Widget`, ex. "glcanvas".
    /// The `Widget` can then be referred to by name
    pub fn new<S: Into<String>>(name: S) -> Self {
        Widget::new_inner(WidgetInner::new(name))
    }

    pub fn from_modifier<T: Component + WidgetModifier>(component: T) -> Self {
        let name: String = T::name();
        let mut widget = Widget::new_inner(WidgetInner::new(name));
        component.apply(&mut widget);
        widget
    }

    pub fn from_modifier_style<C: Component + WidgetModifier + 'static, T: ComponentStyle<Component = C> + Debug + Send>(style: T) -> Self {
        let mut widget = Widget::new(C::name());
        let style = resources().theme.get_modifier_style(Box::new(style), TypeId::of::<T>(), None);
        let component = style.box_component();
        component.apply(&mut widget);
        widget
    }
    pub fn from_modifier_style_class<C: Component + WidgetModifier + 'static, T: ComponentStyle<Component = C> + Debug + Send>(style: T, class: &str) -> Self {
        let mut widget = Widget::new(C::name());
        let style = resources().theme.get_modifier_style(Box::new(style), TypeId::of::<T>(), Some(String::from(class)));
        let component = style.box_component();
        component.apply(&mut widget);
        widget
    }

    pub fn set_draw_state<T: Draw + Component + 'static>(&mut self, draw_state: T) -> &mut Self {
        self.widget_mut().draw_state.set_draw_state(draw_state);
        self
    }

    pub fn set_draw_style<I: Into<DrawStyle>>(&mut self, style: I) -> &mut Self {
        let style = style.into();
        self.widget_mut().draw_state.set_draw_style(style);
        self.update_draw_state();
        self
    }

    pub fn set_cursor_hit_fn<F: Fn(Rect, Point) -> bool + 'static>(&mut self, cursor_hit_fn: F) -> &mut Self {
        self.widget_mut().cursor_hit_fn = Some(Box::new(cursor_hit_fn));
        self
    }

    pub fn add_filter<M: Filter + 'static>(&mut self, filter: M) -> &mut Self {
        self.widget_mut().filters.insert(TypeId::of::<M>(), Box::new(filter));
        self
    }

    fn new_inner(widget: WidgetInner) -> Self {
        let widget_ref = Widget(Rc::new(RefCell::new(widget)));
        event::event(Target::Root, ::ui::RegisterWidget(widget_ref.clone()));
        widget_ref
    }
    pub(super) fn widget_mut(&self) -> RefMut<WidgetInner> {
        self.0.borrow_mut()
    }
    pub(super) fn widget(&self) -> Ref<WidgetInner> {
        self.0.borrow()
    }
    pub fn add_handler<E: 'static, T: EventHandler<E> + 'static>(&mut self, handler: T) -> &mut Self {
        self.add_handler_wrapper(TypeId::of::<E>(), EventHandlerWrapper::new(handler))
    }
    pub fn add_handler_fn<E: 'static, T: FnMut(&E, EventArgs) + 'static>(&mut self, handler: T) -> &mut Self {
        self.add_handler_wrapper(TypeId::of::<E>(), EventHandlerWrapper::new_from_fn(handler))
    }
    fn add_handler_wrapper(&mut self, type_id: TypeId, handler: EventHandlerWrapper) -> &mut Self {
        self.widget_mut().handlers.entry(type_id).or_insert_with(Vec::new)
            .push(Rc::new(RefCell::new(handler)));
        self
    }

    pub fn layout(&mut self) -> LayoutGuardMut {
        event::event(Target::Root, UpdateLayout(self.clone()));
        LayoutGuardMut { guard: self.0.borrow_mut() }
    }
    pub fn layout_vars(&self) -> LayoutVars {
        self.0.borrow().layout.vars
    }
    pub(crate) fn update_bounds(&mut self, var: VarType, value: f32) {
        {
            let mut widget = self.0.borrow_mut();
            match var {
                VarType::Left => widget.bounds.origin.x = value,
                VarType::Top => widget.bounds.origin.y = value,
                VarType::Width => widget.bounds.size.width = value,
                VarType::Height => widget.bounds.size.height = value,
                _ => (),
            }
        }
        self.event(LayoutUpdated);
    }
    pub fn props(&self) -> PropsGuard {
        PropsGuard { guard: self.0.borrow() }
    }
    pub fn add_prop(&mut self, property: Property) {
        if self.0.borrow_mut().props.insert(property) {
            self.props_updated();
            self.update_draw_state();
        }
        for mut child in self.children() {
            child.add_prop(property);
        }
    }
    pub fn remove_prop(&mut self, property: Property) {
        if self.0.borrow_mut().props.remove(&property) {
            self.props_updated();
            self.update_draw_state();
        }
        for mut child in self.children() {
            child.remove_prop(property);
        }
    }
    pub fn draw_state(&mut self) -> DrawStateGuard {
        DrawStateGuard { guard: self.0.borrow_mut() }
    }
    pub fn downgrade(&self) -> WidgetWeak {
        WidgetWeak(Rc::downgrade(&self.0))
    }
    pub fn id(&self) -> WidgetId {
        self.0.borrow().id
    }
    pub fn set_name(&mut self, name: &str) -> &mut Self {
        self.widget_mut().name = name.to_owned();
        self.widget_mut().layout.name = Some(name.to_owned());
        event::event(Target::Root, UpdateLayout(self.clone()));
        self
    }
    pub fn set_debug_color(&mut self, color: Color) -> &mut Self {
        self.widget_mut().debug_color = Some(color);
        self
    }
    pub fn name(&self) -> String {
        self.0.borrow().name.clone()
    }
    pub fn debug_color(&self) -> Option<Color> {
        self.0.borrow().debug_color
    }
    pub fn has_updated(&self) -> bool {
        self.0.borrow().has_updated
    }
    pub fn set_updated(&self, has_updated: bool) {
        self.0.borrow_mut().has_updated = has_updated;
    }
    pub fn bounds(&self) -> Rect {
        self.0.borrow().bounds
    }
    pub fn update<F, T: Draw + 'static>(&mut self, f: F)
        where F: FnOnce(&mut T)
    {
        self.0.borrow_mut().update(f);
        self.event(StateUpdated);
    }

    pub fn update_filter<F, T: Filter + 'static>(&mut self, f: F)
        where F: FnOnce(&mut T)
    {
        self.0.borrow_mut().update_filter(f);
        self.event(StateUpdated);
    }

    pub fn add_child<U: Into<Widget>>(&mut self, child: U) -> &mut Self {
        let mut child = child.into();
        event::event(Target::Root, ::layout::UpdateLayout(child.clone()));
        child.widget_mut().parent = Some(self.downgrade());
        child.widget_mut().props.extend(self.props().iter().cloned());
        self.widget_mut().children.push(child.clone());
        self.layout().add_child(child.layout().deref_mut());
        self.event(::ui::WidgetAttachedEvent);
        self.event(::ui::ChildAttachedEvent(self.id(), child.layout().vars));
        self.event(::ui::ChildrenUpdatedEvent::Added(child));
        self
    }

    pub fn remove_child(&mut self, mut child: Widget) {
        let child_id = child.id();
        self.layout().remove_child(child.layout().deref_mut());
        let mut widget = self.widget_mut();
        if let Some(index) = widget.children.iter().position(|widget| widget.id() == child_id) {
            widget.children.remove(index);
        }
        self.event(::ui::ChildrenUpdatedEvent::Removed(child.clone()));
        child.event(::ui::WidgetDetachedEvent);
        event::event(Target::Root, ::ui::RemoveWidget(child.clone()));
    }

    pub fn remove_widget(&mut self) {
        if let Some(mut parent) = self.parent() {
            parent.remove_child(self.clone());
        }
    }

    pub fn parent(&self) -> Option<Widget> {
        self.widget().parent.as_ref().and_then(|parent| parent.upgrade())
    }

    pub fn children(&self) -> Vec<Widget> {
        self.widget().children.clone()
    }

    pub fn child(&self, name: &str) -> Option<Widget> {
        self.children().iter().find(|child| child.name() == name).cloned()
    }

    pub fn event<T: 'static>(&self, data: T) {
        event::event(Target::Widget(self.clone()), data);
    }
    pub fn event_subtree<T: 'static>(&self, data: T) {
        event::event(Target::SubTree(self.clone()), data);
    }
    pub fn event_bubble_up<T: 'static>(&self, data: T) {
        event::event(Target::BubbleUp(self.clone()), data);
    }
    pub fn trigger_event(&self, ui: &mut Ui, type_id: TypeId, event: &Any) -> bool {
        let handlers = {
            let mut widget = self.0.borrow_mut();
            let mut handlers: Vec<Rc<RefCell<EventHandlerWrapper>>> = Vec::new();
            if let Some(event_handlers) = widget.handlers.get_mut(&type_id) {
                for handler in event_handlers {
                    handlers.push(Rc::clone(handler));
                }
            }
            handlers
        };

        let mut handled = false;
        for event_handler in handlers {
            // will panic in the case of circular handler calls
            let mut handler = event_handler.borrow_mut();
            let event_args = EventArgs {
                widget: self.clone(),
                ui: ui,
                handled: &mut handled,
            };
            handler.handle(event, event_args);
        }
        handled
    }
    pub fn draw(&mut self, crop_to: Rect, renderer: &mut RenderBuilder, debug: bool) {
        self.draw_widget(crop_to, renderer);
        if debug {
            self.draw_debug(renderer);
        }
    }

    pub fn is_under_cursor(&self, cursor: Point) -> bool {
        if let Some(ref cursor_hit_fn) = self.widget().cursor_hit_fn {
            (cursor_hit_fn)(self.bounds(), cursor)
        } else {
            self.bounds().contains(&cursor)
        }
    }

    fn draw_widget(&mut self, crop_to: Rect, renderer: &mut RenderBuilder) {
        let bounds = self.bounds();
        let clip_id = renderer.builder.define_clip(bounds, vec![], None);
        renderer.builder.push_clip_id(clip_id);
        for (_, filter) in &self.widget().filters {
            filter.push(renderer);
        }
        if let Some(draw_state) = self.widget_mut().draw_state.state.as_mut() {
            draw_state.draw(bounds, crop_to, renderer);
        }
        if let Some(crop_to) = crop_to.intersection(&bounds) {
            for child in &mut self.children() {
                child.draw_widget(crop_to, renderer);
            }
        }
        for (_, filter) in &self.widget().filters {
            filter.pop(renderer);
        }
        renderer.builder.pop_clip_id();
    }
    fn draw_debug(&mut self, renderer: &mut RenderBuilder) {
        let color = self.debug_color().unwrap_or(::color::GREEN);
        render::draw_rect_outline(self.bounds(), color, renderer);
        for child in &mut self.children() {
            child.draw_debug(renderer);
        }
    }
    fn props_updated(&self) {
        self.widget_mut().props_updated = true;
    }
    fn update_draw_state(&self) {
        if self.widget().has_updated | self.widget().props_updated | self.widget().draw_state.needs_update() {
            let props = (*self.props()).clone();
            self.widget_mut().draw_state.update(props);
            self.event(StyleUpdated);
            self.event(StateUpdated);
            self.widget_mut().has_updated = true;
            self.widget_mut().props_updated = false;
        }
    }
}

impl PartialEq for Widget {
    fn eq(&self, other: &Widget) -> bool {
        self.id() == other.id()
    }
}

impl Eq for Widget {}

impl Hash for Widget {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl ::std::fmt::Debug for Widget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.widget().name)
    }
}

pub struct LayoutGuard<'a> {
    guard: Ref<'a, WidgetInner>
}

impl<'b> Deref for LayoutGuard<'b> {
    type Target = Layout;
    fn deref(&self) -> &Layout {
        &self.guard.layout
    }
}

impl LayoutRef for Widget {
    fn layout_ref(&self) -> LayoutVars {
        self.layout_vars()
    }
}

pub struct LayoutGuardMut<'a> {
    guard: RefMut<'a, WidgetInner>
}

impl<'b> Deref for LayoutGuardMut<'b> {
    type Target = Layout;
    fn deref(&self) -> &Layout {
        &self.guard.layout
    }
}

impl<'b> DerefMut for LayoutGuardMut<'b> {
    fn deref_mut(&mut self) -> &mut Layout {
        &mut self.guard.layout
    }
}

pub struct PropsGuard<'a> {
    guard: Ref<'a, WidgetInner>
}

impl<'b> Deref for PropsGuard<'b> {
    type Target = PropSet;
    fn deref(&self) -> &PropSet {
        &self.guard.props
    }
}

pub struct DrawStateGuard<'a> {
    guard: RefMut<'a, WidgetInner>
}

impl<'a> DrawStateGuard<'a> {
    pub fn downcast_ref<T: Draw + 'static>(&self) -> Option<&T> {
        if let Some(ref draw_state) = self.guard.draw_state.state {
            draw_state.downcast_ref::<T>()
        } else {
            None
        }
    }
    pub fn style(&mut self) -> Option<&mut DrawStyle> {
        if let Some(ref mut style) = self.guard.draw_state.style {
            Some(style)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct WidgetWeak(Weak<RefCell<WidgetInner>>);

impl WidgetWeak {
    pub fn upgrade(&self) -> Option<Widget> {
        if let Some(widget_ref) = self.0.upgrade() {
            Some(Widget(widget_ref))
        } else {
            None
        }
    }
}

/// Internal Widget representation, usually handled through a `Widget`.
pub(super) struct WidgetInner {
    id: WidgetId,
    pub(super) draw_state: DrawState,
    filters: HashMap<TypeId, Box<Filter>>,
    cursor_hit_fn: Option<Box<Fn(Rect, Point) -> bool>>,
    props: PropSet,
    has_updated: bool,
    props_updated: bool,
    pub(super) layout: Layout,
    pub(super) bounds: Rect,
    name: String,
    debug_color: Option<Color>,
    children: Vec<Widget>,
    parent: Option<WidgetWeak>,
    handlers: HashMap<TypeId, Vec<Rc<RefCell<EventHandlerWrapper>>>>,
}

impl WidgetInner {
    fn new<S: Into<String>>(name: S) -> Self {
        let id = resources().widget_id();
        let name: String = name.into();
        WidgetInner {
            id: id,
            draw_state: DrawState::default(),
            filters: HashMap::new(),
            cursor_hit_fn: None,
            props: PropSet::new(),
            layout: Layout::new(id.0, Some(name.clone())),
            has_updated: true,
            props_updated: true,
            bounds: Rect::zero(),
            name: name,
            debug_color: None,
            children: Vec::new(),
            parent: None,
            handlers: HashMap::new(),
        }
    }
    fn update<F, T: Draw + 'static>(&mut self, f: F)
        where F: FnOnce(&mut T)
    {
        if let Some(ref mut draw_state) = self.draw_state.state {
            self.has_updated = true;
            let state = draw_state.downcast_mut::<T>().expect("Called update on widget with wrong draw_state type");
            f(state);
        }
    }
    fn update_filter<F, T: Filter + Any + 'static>(&mut self, f: F)
        where F: FnOnce(&mut T)
    {
        if let Some(filter) = self.filters.get_mut(&TypeId::of::<T>()) {
            self.has_updated = true;
            f(filter.downcast_mut::<T>().unwrap());
        }
    }
}

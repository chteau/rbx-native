//! `UIPageLayout`: full-size pages in a row or a column, one of them on
//! screen.

use super::*;

/// A page layout with three pages under one container that fills the
/// viewport. Returns `(layout, [page; 3])`.
fn paged(dom: &mut WeakDom, parent: Ref) -> (Ref, [Ref; 3]) {
    let container = frame(dom, parent, udim2(0.0, 0, 0.0, 0), udim2(1.0, 0, 1.0, 0));
    let layout = dom.new_instance("UIPageLayout", "UIPageLayout", Some(container));
    let pages = [0, 1, 2].map(|order| {
        // A `Position`/`Size` the layout is expected to overrule entirely.
        let page = frame(dom, container, udim2(0.3, 9, 0.3, 9), udim2(0.1, 3, 0.1, 3));
        dom.set_property(page, "LayoutOrder", Variant::Int32(order))
            .unwrap();
        page
    });
    (layout, pages)
}

/// Every element's rect, container first then the pages in tree order.
fn laid(dom: &WeakDom) -> Vec<Rect> {
    resolve(&screens(dom), VIEWPORT)
        .into_iter()
        .map(|element| element.rect)
        .collect()
}

#[test]
fn every_page_takes_the_whole_container_whatever_its_own_size_says() {
    let (mut dom, gui) = screen_gui();
    paged(&mut dom, gui);

    for rect in laid(&dom) {
        assert_eq!([rect.width, rect.height], VIEWPORT);
    }
}

#[test]
fn the_first_page_is_on_screen_and_the_rest_wait_beside_it() {
    let (mut dom, gui) = screen_gui();
    paged(&mut dom, gui);

    let rects = laid(&dom);
    // [0] is the container itself; the pages follow in tree order.
    assert_eq!(rects[1].x, 0.0);
    assert_eq!(rects[2].x, VIEWPORT[0]);
    assert_eq!(rects[3].x, VIEWPORT[0] * 2.0);
    assert!(rects.iter().all(|rect| rect.y == 0.0));
}

#[test]
fn current_page_is_the_one_brought_on_screen() {
    let (mut dom, gui) = screen_gui();
    let (layout, pages) = paged(&mut dom, gui);
    dom.set_property(layout, "CurrentPage", Variant::Ref(pages[1]))
        .unwrap();

    let rects = laid(&dom);
    assert_eq!(rects[1].x, -VIEWPORT[0]);
    assert_eq!(rects[2].x, 0.0);
    assert_eq!(rects[3].x, VIEWPORT[0]);
}

#[test]
fn padding_widens_the_gap_between_one_page_and_the_next() {
    let (mut dom, gui) = screen_gui();
    let (layout, _) = paged(&mut dom, gui);
    dom.set_property(
        layout,
        "Padding",
        Variant::UDim(UDim {
            scale: 0.0,
            offset: 20,
        }),
    )
    .unwrap();

    assert_eq!(laid(&dom)[2].x, VIEWPORT[0] + 20.0);
}

#[test]
fn a_vertical_fill_direction_stacks_the_pages_instead() {
    let (mut dom, gui) = screen_gui();
    let (layout, pages) = paged(&mut dom, gui);
    dom.set_property(layout, "FillDirection", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "CurrentPage", Variant::Ref(pages[2]))
        .unwrap();

    let rects = laid(&dom);
    assert!(rects.iter().all(|rect| rect.x == 0.0));
    assert_eq!(rects[1].y, -VIEWPORT[1] * 2.0);
    assert_eq!(rects[3].y, 0.0);
}

#[test]
fn layout_order_decides_which_page_neighbours_which() {
    let (mut dom, gui) = screen_gui();
    let (_, pages) = paged(&mut dom, gui);
    // Reverse the run: the last page in tree order now leads it.
    dom.set_property(pages[2], "LayoutOrder", Variant::Int32(-1))
        .unwrap();

    let rects = laid(&dom);
    assert_eq!(rects[3].x, 0.0);
    assert_eq!(rects[1].x, VIEWPORT[0]);
}

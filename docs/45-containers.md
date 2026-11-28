# 45 Containers: what was measured

Measured 2026-11-27 on a maintainer's machine through Visio's published automation interface (version 16.0, a hidden instance
of its own, nothing kept): containers dropped from the built-in container stencil around plain rectangles drawn for the
purpose. Only cell values and formulas read through that interface are recorded here. How files are read is in
[04](04-visio-import.md).

## What marks a container

A dropped container states `User.msvStructureType = "Container"` and `User.msvShapeCategories = "ContainerStyleDefaults"`,
and its settings in `User.msvSDContainer*` cells: margin (3 mm times the drop scale), resize (1, "as needed", on the one
dropped), locked (`FALSE`), and others for its look.

## Membership

Membership is in the `Relationships` cell of both ends:

| Shape | Formula |
| --- | --- |
| container | `SUM(DEPENDSON(1,<member>,<member>))`, the newest member first |
| member | `SUM(DEPENDSON(4,<container>))` |

A shape that lost its last container, or a container that lost its last member, keeps an empty term (`DEPENDSON(4)`,
`DEPENDSON(1)`). A shape that never belonged to one has no such formula.

| Operation (through automation) | Seen |
| --- | --- |
| Drop a container around two selected shapes | both become members; the container encloses them |
| Add an outside shape as a member, "expand as needed" | both cells updated; the container grows to enclose it |
| Remove that member | both cells updated; the size stays |
| Fit to contents | the container shrinks around its members |
| Delete a member | the container's term no longer names it |
| A member's position changed so that it sticks out (cell write; container set to resize as needed) | it is no longer a member; the container does not grow |
| Duplicate the container (shape or selection) | the members are duplicated with it; the copy and its members name each other; the originals are unchanged |
| Delete the container as a selection | its members are deleted with it |
| Delete the container as a single shape | the members stay, with `DEPENDSON(4)` |
| Disband | the container goes; the members stay, with `DEPENDSON(4)` |
| Container dropped around a container | the outer one lists the inner container and the inner one's members; each of those members names both (outer first) |

## What LayoutCraft does with it

A move (one shape or several) that takes a shape wholly into a container adds it, and one that takes a member from inside
to not wholly inside removes it, as part of the same undo step; locked containers are left alone. Not done: growing a
container to fit what is dropped, marking the container while dragging, lists.

## Not established

- Moving: moving a selection that holds only the container (a page selection or the window's selection) did **not** move the
  members through automation, and they stopped being members. Selecting container and members together moved all and kept the
  membership. Dragging a container in the application is documented to take its contents along (source S20 of
  [43](43-ui-parity.md)); automation did not show that here.
- Dragging a shape into or out of a container in the application (automation moves added no member).
- Copy and paste: pasting after copying a selection of only the container through automation gave one shape whose term held
  unresolved references. What the application's own copy does was not measured.
- Lists, swimlanes and callouts.

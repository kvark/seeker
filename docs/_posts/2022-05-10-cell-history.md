---
layout: post
title: Cell history
---

The _M-beta-0_ model requires doing orientation-independent frequency analysis on the grid, just to find out if something is moving.
This is a very black-boxy approach to movement detection. What if we exploit the fact we know how those cells are generated?

Let's consider the cells to preserve in time and carry their history around. Spawning a new cell would get some product of histories of the parent 3 cells. It can be copied from a random parent, or averaged across all parents. This way, we can track an average lifetime and an average movement direction of a cell. Detecting moving objects becomes much easier, more feasible to implement.

## M-beta-1: Cell History

Instead of considering the grid to be binary (and using fast logical operations on cells in bulk), let's attach a context to each cell:
  - age
  - velocity. Possibly normalized?

We then search for life as manifestation of continuity through change:
![changes graph]({{site.baseurl}}/assets/life-changes-graph.excalidraw.png)

The overall process would be:
  1. Run random simulations
  2. Build histograms of cell changes (for age and velocity)
  3. Consider the histogram with large middle section to be interesting
  4. Classify, deduplicate, and collect isolated samples

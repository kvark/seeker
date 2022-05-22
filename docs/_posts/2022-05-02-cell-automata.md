---
layout: post
title: Cell automata
---

The _M-alpha-0_ model has too many unknowns, but the biggest issue is that, if/when when set up the process, we wouldn't exactly know what to expect or what to look for. If it's obvious than it's not useful. Let's try to build a model based on another example - the Game of Life.

## Find the rules

More generally, we'd want to create is a process for *finding the rules* of GoF, or similar, automatically, in a given space (cellular automata, kernel size, etc). Reasons why this may hypothetically not work:
  
  - **Technical complexity**. Perhaps the search space is going to be unapproachable by the current computer technology?
  - Unclear how to **define the space**, or the rule field to search in. Is there a connection between the rules complexity (e.g. real cosmos is complicated, or so it seems) and the product of life/conciousness in it? Perhaps, the simpler the rules the more primitive is life created in such a world, and we'll not detect anything.
  - How do we **compute entropy**? We aren't looking for the full order (e.g. every cell is occupied), and we aren't looking for pure chaos (i.e. all cells are random). We strike for the balance, an intersection between the *order and chaos*, and we need a metric.


Expanding on the last point, we aren't interested in the first-order stability, i.e. structures that are still:

![life-block](https://upload.wikimedia.org/wikipedia/commons/thumb/9/96/Game_of_life_block_with_border.svg/132px-Game_of_life_block_with_border.svg.png) ![life-boat](https://upload.wikimedia.org/wikipedia/commons/thumb/7/7f/Game_of_life_boat.svg/164px-Game_of_life_boat.svg.png) ![life-beehive](https://upload.wikimedia.org/wikipedia/commons/thumb/6/67/Game_of_life_beehive.svg/196px-Game_of_life_beehive.svg.png).

We are probably interested in all moving objects:

![life-glider](https://upload.wikimedia.org/wikipedia/commons/f/f2/Game_of_life_animated_glider.gif) ![life-LWSS](https://upload.wikimedia.org/wikipedia/commons/3/37/Game_of_life_animated_LWSS.gif)

It's unclear what to do with oscillators, which happen to be in-between of those groups:

![life-blinker](https://upload.wikimedia.org/wikipedia/commons/9/95/Game_of_life_blinker.gif) ![life-beacon](https://upload.wikimedia.org/wikipedia/commons/1/1c/Game_of_life_beacon.gif)

See the wiki page for [Game of Life](https://en.wikipedia.org/wiki/Conway%27s_Game_of_Life) for more examples.

## M-beta-0: Glider Detection Framework

Simplifying the experiment, we can try to seek the answer to QLUE inside the exact Conway's Game of Life (GoL) ruleset. The advantages here are that the environment and rules are very simple, and we have some idea with concrete examples on what to expect.

Steps:
  1. Build a cellular automata
  2. Run on a set of random input configurations
  3. Detect things that move
  4. Confirm if we find the same things interesting as human researchers do

The (3) is definitely solvable but has complications, and it's the core piece of the process. We could train some neural network on GoF and ask it to find similar things. However, this would invalidate the result of (4), because the ML may overfit on the expectations.

We could also analyse the grid in code, say by using Fourier transform for frequency analysis. We'd probably need a variation (or pre-processing) that makes it isotropic (i.e. independent of the orientation).
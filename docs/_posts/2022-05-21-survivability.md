---
layout: post
title: Survivability
---

## Movement

There are gaps in the formulation of _M-beta-0_. The assumption is that we know a-priori which GoF fragments to consider interesting, but we couldn't draw a line between static objects and dynamic, we couldn't classify the oscillators. What is so special about moving? There are things moving around us, which are clearly not so interesting: planets, photons, wind. Is a glider really much different from these effects of nature? If we could answer this question, we'd also understand what to do with oscillators.

At the same time, we can think of interesting things that don't move. Consider a biological cell that just infinitely replicates. Each child is immovable, but it spawns other children around it. Movement isn't a requirement, it's just a likely property of the class of interesting things we are seeking. Why is movement advantegeous?
  - it allows an organism to spread, reducing the risk of extinction
  - it allows to maneuver both towards opportunities and out of danger, but this implies an operational signal system 

All of these factors can be seen as tools for surviving, including the infinite multiplication scenario. So we should change our search criteria from movement to survival.

## Nature progress

Zooming out to a larger scale, nature creates survivable things under the following factors:

![nature progress]({{site.baseurl}}/assets/nature-progress.excalidraw.png)

The _scale_ is akin the raw computational power. Increasing the scale makes all the other things happen more often.

The _time_ allows small products to accumulate, build on top of each other.

The _selection_ is a main filter, it's the kernel of the positive feedback loop for the process. Things that survive have a chance to influence the future things being created. But wait - if we follow this logic, then the "block" structure, or any other still structure, would be the end product of the nature process.

This is where the last component shines - the _random_ factor. It manifests itself in that the reproduction of cells is non-deterministic. There are mutations possible. It also manifests in random destruction of matter, the slow _erosion_ at global scale. These effects make still structures to not survive, either because they aren't able to perfectly replicate to the next iteration, or because they are physically broken by erosion.

## M-beta-2: Focus on Survival

This new discovery changes the way we measure "interest". Instead of looking for movement, we are going to look for things that survive, and we'll apply a random factor to make sure the still structures don't survive for long. From this perspective, gliders aren't that far from oscillators: they are equally dumb and mechanical, just displacing every iteration.

A promising way to introduce randomness is by configuring the initial set of rules based on probabilities. E.g. instead of hard-coding 3 neighbors to create a cell, we can define the birth of a new cell with 95% given 3 neighbors.

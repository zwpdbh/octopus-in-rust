My model of the optimization problem.

1. each node is a new built out unit.
2. the initial node is ACU with start time is zero.
3. to build something means:

- create a new node
- add edges from existing node to the new node to represent those unit built that new unit

4. A edge from node A to node B therefore represent: unit b is built from A
5. multiple edges could from connect from (subset or all) existing nodes to new node

- a edge from factor node to a new unit node
- multiple engineer nodes have directed edge to the same new unit node
- this represent a factory is building node A and multiple engineers are asistant it.

6. each node contains the unit's finish time elapsed
7. the goal is to expand the graph until it reaches one node which is the target node.
8. the tech dependency means:

- a node could be connect to another node if itself or there is existing node could be connected to the node.
- for example, a t2 engineer node could not be connected to the fatboy if there is no other node; unless there is a t3 engineer node is already connect to it.

9. the time elapsed is computed from eco drain model.
10. when new node is built and it is a resource related node, its effect should be added to the current eco.

so the full picture of optimization becomes given current graph and eco, update eco and graph to reach target unit while minimize the elapsed time.

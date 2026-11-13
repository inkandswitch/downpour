# Diagrams

A code block fenced as `rain` or `certflow` becomes a diagram. The fence
names the notation, so the block doesn't need a header line.

```rain
title Revoking two hops downstream
(Doc)-[edit(Doc)]->(Alice)                  : The Doc grants Alice Edit
(Alice)-[edit(Doc)]->(Bob)                  : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)                       : Bob edits the Doc
cd: (Bob)-[read(Doc)]->(Carol)              : Bob gives Carol Read
(Alice)-<revoke>-x[cd]                      : Alice revokes Bob→Carol
```

The same certificates as a snapshot:

```certflow
(Doc)-[edit(Doc)]->(Alice)
(Alice)-[edit(Doc)]->(Bob)
(Bob)-[read(Doc)]->(Carol)
```

Any other code block is left alone:

```rust
let rain = "not a diagram";
```

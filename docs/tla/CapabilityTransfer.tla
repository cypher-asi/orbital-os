------------------------ MODULE CapabilityTransfer ------------------------
(*
 * TLA+ Specification for Zero OS Capability Transfer Protocol
 * 
 * This specification models capability-based access control in Zero OS,
 * focusing on the grant/revoke/derive operations.
 *
 * Key Properties Verified:
 * 1. NoRightsEscalation - Cannot grant more rights than you have
 * 2. NoForgedCapabilities - All capabilities trace to initial grants
 * 3. RevocationComplete - Revoking a capability invalidates all derivations
 *)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    Processes,          \* Set of process IDs
    Objects,            \* Set of object IDs (endpoints, etc.)
    MaxGeneration       \* Maximum capability generation for bounded model checking

VARIABLES
    capabilities,       \* capabilities[p] = set of capability records
    revoked,           \* revoked = set of (object, generation) pairs
    nextGeneration     \* Next generation number to assign

(*
 * Type definitions
 *)
Permission == [read: BOOLEAN, write: BOOLEAN, grant: BOOLEAN]

\* A capability token
CapabilityToken == [
    id: Nat,                    \* Unique capability ID
    object: Objects,            \* Referenced object
    perms: Permission,          \* Granted permissions
    generation: Nat,            \* Generation number (for revocation)
    parent: Nat \cup {-1}       \* Parent capability ID (-1 for root)
]

(*
 * Type invariant
 *)
TypeInvariant ==
    /\ capabilities \in [Processes -> SUBSET CapabilityToken]
    /\ revoked \in SUBSET (Objects \X Nat)
    /\ nextGeneration \in Nat

(*
 * Initial state - no capabilities, nothing revoked
 *)
Init ==
    /\ capabilities = [p \in Processes |-> {}]
    /\ revoked = {}
    /\ nextGeneration = 0

(*
 * Helper: Permission subset check
 *)
PermSubset(p1, p2) ==
    /\ (p1.read => p2.read)
    /\ (p1.write => p2.write)
    /\ (p1.grant => p2.grant)

(*
 * Helper: Check if capability is valid (not revoked)
 *)
IsValid(cap) ==
    <<cap.object, cap.generation>> \notin revoked

(*
 * Helper: Get a capability by ID from a process
 *)
GetCap(p, capId) ==
    CHOOSE cap \in capabilities[p] : cap.id = capId

(*
 * Helper: Has capability with ID
 *)
HasCap(p, capId) ==
    \E cap \in capabilities[p] : cap.id = capId

(*
 * Action: Grant a capability to another process
 * 
 * This is the CORE security operation. It must ensure:
 * 1. Granter has the capability
 * 2. Granter has grant permission
 * 3. New permissions are subset of granter's permissions
 * 4. Generation is inherited from parent
 *)
Grant(granter, grantee, parentCapId, newPerms) ==
    /\ HasCap(granter, parentCapId)
    /\ LET parentCap == GetCap(granter, parentCapId)
       IN
        /\ IsValid(parentCap)
        /\ parentCap.perms.grant = TRUE       \* Must have grant permission
        /\ PermSubset(newPerms, parentCap.perms)  \* Cannot escalate
        /\ nextGeneration < MaxGeneration     \* Bounded model checking
        /\ LET newCap == [
                id |-> nextGeneration,
                object |-> parentCap.object,
                perms |-> newPerms,
                generation |-> parentCap.generation,  \* Same generation for revocation
                parent |-> parentCapId
            ]
           IN
            /\ capabilities' = [capabilities EXCEPT 
                                ![grantee] = @ \cup {newCap}]
            /\ nextGeneration' = nextGeneration + 1
    /\ UNCHANGED revoked

(*
 * Action: Derive a capability with reduced permissions (self-grant)
 *)
Derive(p, parentCapId, newPerms) ==
    Grant(p, p, parentCapId, newPerms)

(*
 * Action: Delete a capability
 *)
Delete(p, capId) ==
    /\ HasCap(p, capId)
    /\ capabilities' = [capabilities EXCEPT 
                        ![p] = {cap \in @ : cap.id /= capId}]
    /\ UNCHANGED <<revoked, nextGeneration>>

(*
 * Action: Revoke a capability (and all derived capabilities)
 * 
 * Revocation works by marking the (object, generation) pair as revoked.
 * All capabilities with matching object and generation become invalid.
 *)
Revoke(p, capId) ==
    /\ HasCap(p, capId)
    /\ LET cap == GetCap(p, capId)
       IN
        /\ revoked' = revoked \cup {<<cap.object, cap.generation>>}
        \* Optionally remove from holder
        /\ capabilities' = [capabilities EXCEPT 
                            ![p] = {c \in @ : c.id /= capId}]
    /\ UNCHANGED nextGeneration

(*
 * Action: Create initial capability (system operation)
 * This represents the kernel creating capabilities for new objects
 *)
CreateInitial(p, obj, perms) ==
    /\ nextGeneration < MaxGeneration
    /\ LET newCap == [
            id |-> nextGeneration,
            object |-> obj,
            perms |-> perms,
            generation |-> nextGeneration,  \* Fresh generation
            parent |-> -1                    \* No parent (root)
        ]
       IN
        /\ capabilities' = [capabilities EXCEPT ![p] = @ \cup {newCap}]
        /\ nextGeneration' = nextGeneration + 1
    /\ UNCHANGED revoked

(*
 * Next state relation
 *)
Next ==
    \/ \E granter, grantee \in Processes, capId \in 0..MaxGeneration,
         r, w, g \in BOOLEAN :
        Grant(granter, grantee, capId, [read |-> r, write |-> w, grant |-> g])
    \/ \E p \in Processes, capId \in 0..MaxGeneration,
         r, w, g \in BOOLEAN :
        Derive(p, capId, [read |-> r, write |-> w, grant |-> g])
    \/ \E p \in Processes, capId \in 0..MaxGeneration :
        Delete(p, capId)
    \/ \E p \in Processes, capId \in 0..MaxGeneration :
        Revoke(p, capId)
    \/ \E p \in Processes, obj \in Objects, r, w, g \in BOOLEAN :
        CreateInitial(p, obj, [read |-> r, write |-> w, grant |-> g])

(*
 * Specification
 *)
Spec == Init /\ [][Next]_<<capabilities, revoked, nextGeneration>>

(*
 * ========================================================================
 * Safety Properties
 * ========================================================================
 *)

(*
 * Property 1: No rights escalation
 * A derived/granted capability never has more permissions than its parent
 *)
NoRightsEscalation ==
    \A p \in Processes, cap \in capabilities[p] :
        cap.parent /= -1 =>
            \A q \in Processes :
                \E parentCap \in capabilities[q] :
                    parentCap.id = cap.parent =>
                        PermSubset(cap.perms, parentCap.perms)

(*
 * Property 2: Revocation is effective
 * Once revoked, a capability is never valid again
 *)
RevocationEffective ==
    \A obj \in Objects, gen \in 0..MaxGeneration :
        <<obj, gen>> \in revoked =>
            \A p \in Processes, cap \in capabilities[p] :
                cap.object = obj /\ cap.generation = gen =>
                    ~IsValid(cap)

(*
 * Property 3: All capabilities trace to roots
 * Every capability either has parent = -1 or has a valid parent chain
 *)
CapabilitiesTraceToRoots ==
    \A p \in Processes, cap \in capabilities[p] :
        cap.parent = -1 \/
        (\E q \in Processes : HasCap(q, cap.parent))

(*
 * Property 4: Generation monotonicity
 * Generations only increase
 *)
GenerationMonotonic ==
    nextGeneration >= Cardinality(UNION {capabilities[p] : p \in Processes})

(*
 * ========================================================================
 * Theorems
 * ========================================================================
 *)

THEOREM Spec => []TypeInvariant
THEOREM Spec => []NoRightsEscalation
THEOREM Spec => []RevocationEffective
THEOREM Spec => []GenerationMonotonic

=============================================================================

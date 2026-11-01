/**
 * The strings of the worktree panel's end-of-cycle cleanup: removing, in one
 * go, the worktrees whose branch is already merged. i18n.ts merges both
 * dictionaries into its own, so a key reads through `t()` like any other; keys
 * start with `worktrees.cleanup`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled. French follows the glossary and
 * the typography of i18n.ts: plain spaces before ? ! ; : » and after «, the
 * typesetter does the rest.
 */

export const en = {
  "worktrees.cleanup": "Clean up merged…",
  "worktrees.cleanupHint": "Removes, in one go, the worktrees whose branch is already merged",
  "worktrees.cleanupTitle": "Merged worktrees",
  "worktrees.cleanupIntro":
    "These worktrees sit on a branch already merged into the main checkout. The ones that look finished are ticked.",
  "worktrees.cleanupNone": "No worktree sits on a merged branch.",
  "worktrees.cleanupLoading": "Looking for merged worktrees…",
  "worktrees.cleanupNoOwnCommit": "no commit of its own",
  "worktrees.cleanupNoOwnCommitHint":
    "Its branch is at the same commit as the main checkout, so git counts it as merged — it may be a worker that has just started",
  "worktrees.cleanupHeld": "open in {name}",
  "worktrees.cleanupHeldHint": "A pane of Hangar is open in this worktree",
  "worktrees.cleanupDirty_one": "{n} file not committed",
  "worktrees.cleanupDirty_other": "{n} files not committed",
  "worktrees.cleanupDirtyUnknown": "state unknown",
  "worktrees.cleanupLockedHint": "Locked: unlock it first to remove it",
  "worktrees.cleanupRemove_one": "Remove {n} worktree",
  "worktrees.cleanupRemove_other": "Remove {n} worktrees",
  "worktrees.cleanupConfirm_one": "Confirm: remove {n} worktree?",
  "worktrees.cleanupConfirm_other": "Confirm: remove {n} worktrees?",
  "worktrees.cleanupConfirmDirty":
    "{n} of them hold uncommitted work: git will refuse to remove them, and you will be asked again for each.",
  "worktrees.cleanupRemoving": "Removing…",
  "worktrees.cleanupRemoved_one": "{n} worktree removed",
  "worktrees.cleanupRemoved_other": "{n} worktrees removed",
  "worktrees.cleanupNeedsForce": "Held back by git — force removes what they hold:",
  "worktrees.cleanupFailed": "Not removed:",
  "worktrees.cleanupBranches": "Their branches are merged and no worktree holds them any more. Delete them?",
  "worktrees.cleanupBranchesDelete": "Delete the branches",
  "worktrees.cleanupBranchesKeep": "Keep them",
  "worktrees.cleanupBranchesForce": "Not fully merged — force (-D) these?",
  "worktrees.cleanupBranchesDone_one": "{n} branch deleted",
  "worktrees.cleanupBranchesDone_other": "{n} branches deleted",
  "worktrees.cleanupBack": "Back to the list",
};

export const fr: Record<keyof typeof en, string> = {
  "worktrees.cleanup": "Nettoyer les mergés…",
  "worktrees.cleanupHint": "Supprime en une fois les worktrees dont la branche est déjà mergée",
  "worktrees.cleanupTitle": "Worktrees mergés",
  "worktrees.cleanupIntro":
    "Ces worktrees reposent sur une branche déjà mergée dans le checkout principal. Ceux qui semblent terminés sont cochés.",
  "worktrees.cleanupNone": "Aucun worktree ne repose sur une branche mergée.",
  "worktrees.cleanupLoading": "Recherche des worktrees mergés…",
  "worktrees.cleanupNoOwnCommit": "aucun commit propre",
  "worktrees.cleanupNoOwnCommitHint":
    "Sa branche est au même commit que le checkout principal, donc git la compte comme mergée — ce peut être un worker qui vient de commencer",
  "worktrees.cleanupHeld": "ouvert dans {name}",
  "worktrees.cleanupHeldHint": "Un pane de Hangar est ouvert dans ce worktree",
  "worktrees.cleanupDirty_one": "{n} fichier non commité",
  "worktrees.cleanupDirty_other": "{n} fichiers non commités",
  "worktrees.cleanupDirtyUnknown": "état inconnu",
  "worktrees.cleanupLockedHint": "Verrouillé : déverrouillez-le d'abord pour le supprimer",
  "worktrees.cleanupRemove_one": "Supprimer {n} worktree",
  "worktrees.cleanupRemove_other": "Supprimer {n} worktrees",
  "worktrees.cleanupConfirm_one": "Confirmer : supprimer {n} worktree ?",
  "worktrees.cleanupConfirm_other": "Confirmer : supprimer {n} worktrees ?",
  "worktrees.cleanupConfirmDirty":
    "{n} d'entre eux contiennent du travail non commité : git refusera de les supprimer, et une confirmation sera redemandée pour chacun.",
  "worktrees.cleanupRemoving": "Suppression…",
  "worktrees.cleanupRemoved_one": "{n} worktree supprimé",
  "worktrees.cleanupRemoved_other": "{n} worktrees supprimés",
  "worktrees.cleanupNeedsForce": "Retenus par git — forcer supprime ce qu'ils contiennent :",
  "worktrees.cleanupFailed": "Non supprimés :",
  "worktrees.cleanupBranches": "Leurs branches sont mergées et plus aucun worktree ne les tient. Les supprimer ?",
  "worktrees.cleanupBranchesDelete": "Supprimer les branches",
  "worktrees.cleanupBranchesKeep": "Les garder",
  "worktrees.cleanupBranchesForce": "Pas entièrement mergées — forcer (-D) pour celles-ci ?",
  "worktrees.cleanupBranchesDone_one": "{n} branche supprimée",
  "worktrees.cleanupBranchesDone_other": "{n} branches supprimées",
  "worktrees.cleanupBack": "Retour à la liste",
};

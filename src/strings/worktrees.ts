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
  "worktrees.migrate": "Bring changes back",
  "worktrees.migrateHint": "Moves this worktree's uncommitted changes into the main checkout",
  "worktrees.migrateConfirm": "Confirm?",
  "worktrees.migrateWarn":
    "This empties the uncommitted changes of this worktree and applies them in the main checkout. Staged files come across as plain changes.",
  "worktrees.migrateCancel": "Cancel",
  "worktrees.migrateDone": "Changes brought back to the main checkout.",
  "worktrees.migrateNothing": "This worktree has no uncommitted change.",
  "worktrees.migrateConflict":
    "Modified both here and in the main checkout, nothing was moved:",
  "worktrees.migrateMore_one": "and {n} more",
  "worktrees.migrateMore_other": "and {n} more",
  "worktrees.migrateFailed":
    "The changes could not be applied in the main checkout. Nothing is lost: they are kept in a stash. Get them back with: git stash apply {stash}",
  "worktrees.migrateDropFailed":
    "Changes brought back, but their stash could not be dropped. It is harmless; remove it with git stash drop when you want.",
  "worktrees.hooksTitle": "Worktree commands",
  "worktrees.hooksRunsHere":
    "These commands run on this machine, as you, through your default shell. Only you can set them: no agent can.",
  "worktrees.hooksPostCreate": "After creating a worktree",
  "worktrees.hooksPostCreateHint":
    "Runs in the new worktree, for the panel's and for a worker's, with WORKTREE_PATH and BASE_PATH set. If it fails the worktree is kept and the failure is shown (and told to the worker).",
  "worktrees.hooksPreRemove": "Before removing a worktree",
  "worktrees.hooksPreRemoveHint":
    "Runs in the worktree first, also when cleaning up merged ones. If it fails or runs past 5 minutes the worktree is kept, and you can remove it anyway.",
  "worktrees.hooksPlaceholder": "Empty runs nothing — e.g. pnpm install",
  "worktrees.hookPostDone": "The postCreate command ran.",
  "worktrees.hookPostFailed": "The postCreate command {problem}. The worktree was kept.",
  "worktrees.hookRemoveAnyway": "Remove anyway",
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
  "worktrees.migrate": "Rapatrier les changements",
  "worktrees.migrateHint": "Déplace les changements non commités de ce worktree vers le checkout principal",
  "worktrees.migrateConfirm": "Confirmer ?",
  "worktrees.migrateWarn":
    "Cela vide les changements non commités de ce worktree et les applique dans le checkout principal. Les fichiers indexés arrivent comme de simples modifications.",
  "worktrees.migrateCancel": "Annuler",
  "worktrees.migrateDone": "Changements rapatriés dans le checkout principal.",
  "worktrees.migrateNothing": "Ce worktree n'a aucun changement non commité.",
  "worktrees.migrateConflict":
    "Modifiés ici et dans le checkout principal, rien n'a été déplacé :",
  "worktrees.migrateMore_one": "et {n} autre",
  "worktrees.migrateMore_other": "et {n} autres",
  "worktrees.migrateFailed":
    "Les changements n'ont pas pu être appliqués dans le checkout principal. Rien n'est perdu : ils sont gardés dans un stash. Pour les récupérer : git stash apply {stash}",
  "worktrees.migrateDropFailed":
    "Changements rapatriés, mais leur stash n'a pas pu être supprimé. Il est sans danger ; retirez-le avec git stash drop quand vous voulez.",
  "worktrees.hooksTitle": "Commandes de worktree",
  "worktrees.hooksRunsHere":
    "Ces commandes s'exécutent sur cette machine, sous votre compte, via votre shell par défaut. Vous seul pouvez les définir : aucun agent ne le peut.",
  "worktrees.hooksPostCreate": "Après la création d'un worktree",
  "worktrees.hooksPostCreateHint":
    "S'exécute dans le nouveau worktree, celui du panneau comme celui d'un worker, avec WORKTREE_PATH et BASE_PATH définis. En cas d'échec le worktree est gardé et l'échec est affiché (et signalé au worker).",
  "worktrees.hooksPreRemove": "Avant la suppression d'un worktree",
  "worktrees.hooksPreRemoveHint":
    "S'exécute d'abord dans le worktree, y compris au nettoyage des worktrees fusionnés. En cas d'échec ou au-delà de 5 minutes, le worktree est gardé et vous pouvez le supprimer quand même.",
  "worktrees.hooksPlaceholder": "Vide : rien ne s'exécute — par ex. pnpm install",
  "worktrees.hookPostDone": "La commande postCreate s'est exécutée.",
  "worktrees.hookPostFailed": "La commande postCreate {problem}. Le worktree est conservé.",
  "worktrees.hookRemoveAnyway": "Supprimer quand même",
};

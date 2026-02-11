# OBS-002 Stable element ids across snapshots

- Element ids now hash stable accessibility identity (AX node id + optional frame id)
  instead of snapshot position or backend DOM node ids.
- Id generation no longer depends on occurrence counts, so minor layout shifts or
  reordering do not churn ids on static pages.
- Backend node ids remain mapped for action execution, but do not influence the
  public element id anymore.

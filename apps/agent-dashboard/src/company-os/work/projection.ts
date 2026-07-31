export interface BusinessLineWorkItem {
  id: string;
  businessLineId?: string;
}

export interface BusinessLineDimension {
  id: string;
  label: string;
  workItemIds: string[];
}

export interface BusinessLineProjection {
  dimensions: BusinessLineDimension[];
  integrityFindings: string[];
}

export interface AcceptanceCriteriaPresentation {
  label: string;
  semantic: "criteria_count";
  tone: "neutral";
  workItemStatus: string;
}

export function acceptanceCriteriaPresentation(
  count: number,
  workItemStatus: string,
): AcceptanceCriteriaPresentation {
  return {
    label: `${count} acceptance criteria`,
    semantic: "criteria_count",
    tone: "neutral",
    workItemStatus,
  };
}

export function projectBusinessLineDimensions(
  raw: Record<string, unknown>,
  items: BusinessLineWorkItem[],
  moduleNames: ReadonlyMap<string, string>,
): BusinessLineProjection {
  const itemById = new Map(items.map((item) => [item.id, item]));
  const integrityFindings: string[] = [];
  const dimensions = Object.entries(raw).map(([id, refs]) => {
    const workItemIds = Array.isArray(refs)
      ? refs.filter((ref): ref is string => typeof ref === "string" && ref.length > 0)
      : [];
    for (const workItemId of workItemIds) {
      const item = itemById.get(workItemId);
      if (!item) {
        integrityFindings.push(`business_lines.${id} references missing WorkItem ${workItemId}.`);
      } else if (item.businessLineId !== id && !(id === "unclassified" && !item.businessLineId)) {
        integrityFindings.push(
          `business_lines.${id} references WorkItem ${workItemId} whose business_module_ref is ${item.businessLineId ?? "absent"}.`,
        );
      }
    }
    return {
      id,
      label: moduleNames.get(id) ?? id,
      workItemIds,
    };
  });
  return { dimensions, integrityFindings };
}

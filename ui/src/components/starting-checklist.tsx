'use client';

interface ChecklistItem {
  label: string;
  status: 'done' | 'in_progress' | 'pending';
}

const CHECKLIST_ITEMS: ChecklistItem[] = [
  { label: 'Configuration validated', status: 'done' },
  { label: 'Metrics availability confirmed', status: 'done' },
  { label: 'Layer allocation confirmed', status: 'done' },
  { label: 'Traffic ramp in progress', status: 'in_progress' },
];

const STATUS_LABELS: Record<ChecklistItem['status'], string> = {
  done: 'Completed',
  in_progress: 'In progress',
  pending: 'Pending',
};

export function StartingChecklist() {
  return (
    <div
      className="rounded-lg border border-yellow-200 bg-yellow-50 p-4"
      role="region"
      aria-label="Experiment startup progress"
    >
      <h3 className="text-sm font-semibold text-yellow-800">Starting Experiment</h3>
      <p className="mt-1 text-xs text-yellow-700">
        Validating configuration and ramping traffic...
      </p>
      <ul className="mt-3 space-y-2" aria-label="Startup checklist">
        {CHECKLIST_ITEMS.map((item) => (
          <li key={item.label} className="flex items-center gap-2 text-sm">
            {item.status === 'done' && (
              <span
                className="text-green-600"
                data-testid="check-done"
                title="Completed"
                aria-hidden="true"
              >
                &#10003;
              </span>
            )}
            {item.status === 'in_progress' && (
              <span
                className="inline-block h-3 w-3 animate-pulse rounded-full bg-yellow-500"
                data-testid="check-progress"
                title="In progress"
                aria-hidden="true"
              />
            )}
            {item.status === 'pending' && (
              <span
                className="inline-block h-3 w-3 rounded-full border-2 border-gray-300"
                title="Pending"
                aria-hidden="true"
              />
            )}
            <span className={item.status === 'done' ? 'text-gray-600' : 'text-gray-900'}>
              {item.label}
            </span>
            <span className="sr-only">({STATUS_LABELS[item.status]})</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

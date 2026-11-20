"""Audit wall time without attributing operator latency or host suspension to work."""
import re
from datetime import datetime


def union_ms(intervals):
    total, end = 0, None
    for start, finish in sorted(intervals):
        if finish < start:
            raise ValueError('Reversed timing interval')
        total += max(0, finish - max(start, end if end is not None else start))
        end = max(finish, end if end is not None else finish)
    return total


def approval_windows(steps, confirmations, records):
    by_id = {identity: (data, at) for identity, _, _, data, at in records}
    windows = []
    for confirmation in confirmations:
        step = steps.get(confirmation['execution'])
        if step is None:
            continue
        _, signed = by_id[confirmation['record_id']]
        _, requested = by_id[confirmation['request_id']]
        ready = step['finished']
        if not ready <= requested <= signed:
            raise ValueError('Approval timestamps are out of order')
        windows.append({'step': step['step'], 'execution': step['id'],
            'request_id': confirmation['request_id'], 'ready_at_ms': ready,
            'requested_at_ms': requested, 'signed_at_ms': signed,
            'manager_preparation_ms': requested - ready,
            'external_response_ms': signed - requested})
    return sorted(windows, key=lambda window: window['ready_at_ms'])


def suspension_windows(power_log, events, start, finish):
    """Pair OS sleep/wake evidence; clip transition seconds around actual DB activity.

    Power logs have second precision and describe transitions, not the exact instant
    a process stops. Only the event-free portion between the last pre-suspend and
    first post-resume event is excluded; active transition time remains charged.
    This is an explicit estimate, never an inference from an event gap alone.
    """
    pattern = re.compile(r'^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d [+-]\d{4})\s+(Sleep|DarkWake|Wake)[ \t]{2,}(.*)$')
    pending, result = None, []
    times = sorted(set(events))
    for line in power_log.splitlines():
        match = pattern.match(line)
        if not match:
            continue
        timestamp, kind, _ = match.groups()
        at = round(datetime.strptime(timestamp, '%Y-%m-%d %H:%M:%S %z').timestamp() * 1000)
        if kind == 'Sleep':
            pending = (at, line)
        elif pending is not None:
            began, source = pending
            pending = None
            if at <= start or began >= finish:
                continue
            # Sleep notification completion may follow the entry log by seconds.
            # Choose the latest event in that transition, not the earlier log time.
            before = max((event for event in times if event <= began + 2000), default=began)
            after = min((event for event in times if event >= at - 1000), default=at)
            excluded_start = max(start, began, before)
            excluded_end = min(finish, at, after)
            if excluded_end <= excluded_start:
                continue
            if any(excluded_start < event < excluded_end for event in times):
                raise ValueError('Database activity inside proposed host suspension')
            result.append({'os_sleep_at_ms': began, 'os_wake_at_ms': at,
                'started_at_ms': excluded_start, 'finished_at_ms': excluded_end,
                'duration_ms': excluded_end - excluded_start,
                'precision': 'OS log seconds; event-bounded transition estimate',
                'source': [source, line]})
    return result


def audit(start, finish, approvals, suspensions):
    responses = [(item['requested_at_ms'], item['signed_at_ms']) for item in approvals]
    pauses = [(item['started_at_ms'], item['finished_at_ms']) for item in suspensions]
    excluded = union_ms(responses + pauses)
    return {'raw_wall_ms': finish - start,
        'manager_approval_preparation_ms': sum(item['manager_preparation_ms'] for item in approvals),
        'external_approval_response_ms': union_ms(responses),
        'host_suspension_ms': union_ms(pauses), 'excluded_union_ms': excluded,
        'workflow_net_ms': finish - start - excluded,
        'approvals': approvals, 'host_suspensions': suspensions,
        'completion_observation_delay_included': False,
        'notes': ['Net time excludes the union of external approval response and evidenced host suspension.',
            'Approval response includes the delegated operator reading/reviewing the approval packet.',
            'Manager preparation, dispatch, acceptance, Git, tests and explicit completion remain workflow time.',
            'Host suspension boundaries are estimates limited by second-resolution OS transition logs.',
            'Persisted completion/worker settlement timestamps define the end; polling/discovery time is never used.']}

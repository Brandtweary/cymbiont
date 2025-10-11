#!/usr/bin/env python3
"""
Dual-Context Knowledge Graph Injection Hook

This hook queries Graphiti FastAPI server with both user and agent messages in parallel,
injecting relevant nodes and facts as structured XML context. The dual-context
approach enables closed-loop feedback where both participants steer knowledge
exploration, with agent responses automatically loading relevant context for
follow-ups. Context is ephemeral and updates in place each turn.
"""

import asyncio
import json
import sys
from pathlib import Path
from typing import Any

import requests


# Simple data classes for graph results
class Node:
    """Simple node container matching Graphiti node structure."""
    def __init__(self, uuid: str, name: str, summary: str):
        self.uuid = uuid
        self.name = name
        self.summary = summary


class Edge:
    """Simple edge container matching Graphiti edge structure."""
    def __init__(self, uuid: str, name: str, fact: str, source_node_uuid: str, target_node_uuid: str,
                 source_node_name: str = None, target_node_name: str = None):
        self.uuid = uuid
        self.name = name
        self.fact = fact
        self.source_node_uuid = source_node_uuid
        self.target_node_uuid = target_node_uuid
        self.source_node_name = source_node_name or source_node_uuid
        self.target_node_name = target_node_name or target_node_uuid


async def query_knowledge_graph(query_text: str, node_limit: int = 6, edge_limit: int = 12) -> tuple[list[Any], list[Any]]:
    """
    Query Graphiti FastAPI server for relevant nodes and facts.

    Args:
        query_text: Search query
        node_limit: Number of nodes to fetch (default 6 for agent queries with dedup headroom)
        edge_limit: Number of edges to fetch (default 12 for agent queries with dedup headroom)

    Returns:
        Tuple of (nodes, edges)
    """
    try:
        # Query nodes via FastAPI
        node_response = requests.post(
            "http://localhost:8000/search/nodes",
            json={"query": query_text, "max_nodes": node_limit}
        )
        node_response.raise_for_status()
        node_data = node_response.json()

        # Query facts via FastAPI
        fact_response = requests.post(
            "http://localhost:8000/search",
            json={"query": query_text, "max_facts": edge_limit}
        )
        fact_response.raise_for_status()
        fact_data = fact_response.json()

        # Convert JSON responses to Node/Edge objects
        nodes = [Node(uuid=n["uuid"], name=n["name"], summary=n["summary"])
                 for n in node_data.get("nodes", [])]

        edges = [Edge(
            uuid=e["uuid"],
            name=e["name"],
            fact=e["fact"],
            source_node_uuid=e.get("source_node_uuid", ""),
            target_node_uuid=e.get("target_node_uuid", ""),
            source_node_name=e.get("source_node_name"),
            target_node_name=e.get("target_node_name")
        ) for e in fact_data.get("facts", [])]

        return nodes, edges

    except Exception as e:
        # Log error to stderr, return empty results (terse to avoid context bloat)
        print(f"KG query error: {type(e).__name__}", file=sys.stderr)
        return [], []


def get_last_assistant_message(transcript_path: str) -> str | None:
    """
    Parse transcript JSONL file and extract the last assistant message.

    Returns:
        Content of last assistant message, or None if not found
    """
    try:
        if not os.path.exists(transcript_path):
            return None

        last_assistant_msg = None

        with open(transcript_path, 'r') as f:
            for line in f:
                try:
                    entry = json.loads(line.strip())
                    if entry.get('type') == 'assistant':
                        # Extract message content (always a list of content blocks for Claude messages)
                        msg = entry.get('message', {})
                        content = msg.get('content', [])

                        # Extract text from content blocks
                        text_parts = []
                        for block in content:
                            if isinstance(block, dict) and block.get('type') == 'text':
                                text = block.get('text', '')
                                if text:
                                    text_parts.append(text)

                        if text_parts:
                            last_assistant_msg = '\n'.join(text_parts)

                except json.JSONDecodeError:
                    continue

        return last_assistant_msg

    except Exception as e:
        print(f"Error reading transcript: {e}", file=sys.stderr)
        return None


async def query_dual_context(user_msg: str, agent_msg: str | None) -> tuple[list[Any], list[Any], list[Any], list[Any]]:
    """
    Query knowledge graph with both user and agent messages in parallel.

    User query fetches target count (3n/6e) - no deduplication needed.
    Agent query fetches 2× target (6n/12e) - provides headroom for deduplication backfill.

    Returns:
        Tuple of (user_nodes, user_edges, agent_nodes, agent_edges)
    """
    if agent_msg:
        # Parallel queries: user gets exact count, agent gets 2× for deduplication headroom
        (user_nodes, user_edges), (agent_nodes, agent_edges) = await asyncio.gather(
            query_knowledge_graph(user_msg, node_limit=3, edge_limit=6),
            query_knowledge_graph(agent_msg, node_limit=6, edge_limit=12)
        )
    else:
        # No agent message, only query user context
        user_nodes, user_edges = await query_knowledge_graph(user_msg, node_limit=3, edge_limit=6)
        agent_nodes, agent_edges = [], []

    return user_nodes, user_edges, agent_nodes, agent_edges


def deduplicate_with_backfill(
    user_results: list[Any],
    agent_results: list[Any],
    target_count: int
) -> tuple[list[Any], list[Any], dict[str, int]]:
    """
    Deduplicate results with user-priority backfill.

    User context gets top N results guaranteed.
    Agent context gets top N unique results (skipping duplicates, going deeper).

    Args:
        user_results: User query results (should be 2×target_count)
        agent_results: Agent query results (should be 2×target_count)
        target_count: Target count per context (e.g., 3 for nodes, 6 for edges)

    Returns:
        Tuple of (user_deduplicated, agent_deduplicated, stats)
    """
    # User context: take top N results
    user_final = user_results[:target_count]
    user_uuids = {item.uuid for item in user_final}

    # Agent context: take top N unique results (skip user duplicates, backfill from deeper results)
    agent_final = []
    duplicates_skipped = 0
    for item in agent_results:
        if item.uuid not in user_uuids:
            agent_final.append(item)
            if len(agent_final) >= target_count:
                break
        else:
            duplicates_skipped += 1

    stats = {
        'duplicates_removed': duplicates_skipped,
        'backfilled_count': len(agent_final)
    }

    return user_final, agent_final, stats


def format_context_section(nodes: list[Any], edges: list[Any]) -> list[str]:
    """
    Format a single context section (user or agent).

    Returns:
        List of formatted lines
    """
    lines = []

    # Format nodes
    if nodes:
        lines.append("Nodes:")
        for node in nodes:
            lines.append(f"- [{node.name}]: {node.summary}")
    else:
        lines.append("Nodes: (none found)")

    lines.append("")  # blank line

    # Format facts/edges
    if edges:
        lines.append("Facts:")
        for edge in edges:
            # Get source and target node names if available
            source_name = getattr(edge, 'source_node_name', edge.source_node_uuid)
            target_name = getattr(edge, 'target_node_name', edge.target_node_uuid)
            fact = edge.fact
            name = edge.name

            lines.append(f"- [{source_name}] → [{name}] → [{target_name}]: {fact}")
    else:
        lines.append("Facts: (none found)")

    return lines


def format_xml_context(
    user_nodes: list[Any],
    user_edges: list[Any],
    agent_nodes: list[Any],
    agent_edges: list[Any]
) -> str:
    """
    Format dual-context query results as XML for injection into Claude's context.
    """
    xml_parts = ["<knowledge-graph>"]

    # User context section
    xml_parts.append("<user-context>")
    xml_parts.extend(format_context_section(user_nodes, user_edges))
    xml_parts.append("</user-context>")

    xml_parts.append("")  # blank line between sections

    # Agent context section (only if agent results exist)
    if agent_nodes or agent_edges:
        xml_parts.append("<agent-context>")
        xml_parts.extend(format_context_section(agent_nodes, agent_edges))
        xml_parts.append("</agent-context>")

    xml_parts.append("</knowledge-graph>")

    return "\n".join(xml_parts)


def main():
    """Main hook entry point."""
    try:
        # Read hook input from stdin
        input_data = json.load(sys.stdin)
        user_prompt = input_data.get('prompt', '')

        # TODO: Remove this workaround once Claude Code fixes stale session_id bug
        # WORKAROUND: Claude Code bug passes stale session ID after reload
        # Manually find most recently modified transcript
        import glob
        import os
        from pathlib import Path

        session_dir = Path.home() / ".claude/projects/-home-brandt-projects-hector"
        transcripts = glob.glob(str(session_dir / "*.jsonl"))
        if transcripts:
            transcript_path = max(transcripts, key=os.path.getmtime)
        else:
            # Fallback to hook input if no transcripts found
            transcript_path = input_data.get('transcript_path', '')

        if not user_prompt:
            # No prompt to query, return empty context
            sys.exit(0)

        # Get last assistant message from transcript
        agent_msg = get_last_assistant_message(transcript_path) if transcript_path else None

        # Query knowledge graph with dual context (fetches 2x results for deduplication)
        user_nodes_raw, user_edges_raw, agent_nodes_raw, agent_edges_raw = asyncio.run(
            query_dual_context(user_prompt, agent_msg)
        )

        # Deduplicate with user-priority backfill
        user_nodes, agent_nodes, _ = deduplicate_with_backfill(user_nodes_raw, agent_nodes_raw, target_count=3)
        user_edges, agent_edges, _ = deduplicate_with_backfill(user_edges_raw, agent_edges_raw, target_count=6)

        # Format and print XML context
        xml_context = format_xml_context(user_nodes, user_edges, agent_nodes, agent_edges)
        print(xml_context)

        # Exit successfully - stdout will be injected as additional context
        sys.exit(0)

    except Exception as e:
        # Log error to stderr and exit successfully with empty context
        # Keep terse to avoid context bloat if errors get captured
        print(f"KG hook error: {type(e).__name__}", file=sys.stderr)
        sys.exit(0)


if __name__ == "__main__":
    main()

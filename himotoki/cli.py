"""
Command line interface for himotoki.

Run 'himotoki --help' for usage information.
"""

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Optional

from himotoki.db.connection import get_session, get_db_path
from himotoki.output import (
    dict_segment, simple_segment,
    segment_to_json, segment_to_text,
    WordType, word_info_reading_str, get_senses_str, get_root_seq,
    get_conj_description, get_entry_reading,
    format_conjugation_info, _get_conjugation_display,
)
from himotoki.characters import romanize_word

# =============================================================================
# Constants
# =============================================================================

VERSION = '0.3.2'
CONJUGATION_ROOT = 'root'  # Marker for unconjugated (dictionary) form


def get_kana(wi) -> str:
    """Extract kana reading from a WordInfo object.
    
    Handles the various formats kana can be stored in:
    - str: return as-is
    - list: return first element
    - None/empty: fall back to text
    """
    if isinstance(wi.kana, str):
        return wi.kana
    if wi.kana:  # non-empty list
        return wi.kana[0]
    return wi.text or ''


def format_word_info_text(session, word_infos, include_romanization: bool = True) -> str:
    """Format word info list as text output.
    
    Args:
        session: Database session
        word_infos: List of WordInfo objects
        include_romanization: If True, include romanized reading line at top
    """
    lines = []
    
    # Romanized reading line (optional)
    if include_romanization:
        romanized_parts = [romanize_word(get_kana(wi)) for wi in word_infos]
        lines.append(' '.join(romanized_parts))
    
    # Individual word info
    for wi in word_infos:
        if wi.type == WordType.GAP:
            continue
        
        lines.append('')
        
        if include_romanization:
            romanized = romanize_word(get_kana(wi))
            lines.append(f"* {romanized}  {word_info_reading_str(wi)}")
        else:
            lines.append(f"* {word_info_reading_str(wi)}")
        
        if wi.seq:
            senses = get_senses_str(session, wi.seq)
            if not senses:
                # Conjugated entry with no own senses — try root entry
                first_seq = wi.seq if isinstance(wi.seq, int) else (wi.seq[0] if wi.seq else None)
                if first_seq:
                    root_seq = get_root_seq(session, first_seq)
                    if root_seq:
                        senses = get_senses_str(session, root_seq)
            if senses:
                lines.append(senses)
        
        # Conjugation info (breakdown tree)
        conj_strs = _get_conjugation_display(session, wi)
        for cs in conj_strs:
            lines.append(cs)
    
    return '\n'.join(lines)


# =============================================================================
# Output Handlers
# =============================================================================

def output_json(session, text: str, limit: int) -> None:
    """Output segmentation as JSON."""
    from himotoki.output import word_info_gloss_json
    
    results = dict_segment(session, text, limit=limit)
    output = []
    for word_infos, score in results:
        segments = []
        for wi in word_infos:
            romanized = romanize_word(get_kana(wi))
            segment_json = word_info_gloss_json(session, wi)
            segments.append([romanized, segment_json, []])
        output.append([segments, score])
    
    print(json.dumps(output, ensure_ascii=False))


def output_romanize(session, text: str) -> None:
    """Output simple romanization."""
    results = dict_segment(session, text, limit=1)
    if not results:
        print(text)
        return
    
    word_infos, _ = results[0]
    romanized_parts = [romanize_word(get_kana(wi)) for wi in word_infos]
    print(' '.join(romanized_parts))


def output_full(session, text: str) -> None:
    """Output romanization with dictionary info."""
    results = dict_segment(session, text, limit=1)
    if not results:
        print(text)
        return
    
    word_infos, _ = results[0]
    print(format_word_info_text(session, word_infos, include_romanization=True))


def output_kana(session, text: str) -> None:
    """Output kana reading with spaces."""
    results = dict_segment(session, text, limit=1)
    if not results:
        print(text)
        return
    
    word_infos, _ = results[0]
    kana_parts = [get_kana(wi) for wi in word_infos]
    print(' '.join(kana_parts))


def output_default(session, text: str) -> None:
    """Output dictionary info only (no romanization)."""
    results = dict_segment(session, text, limit=1)
    if not results:
        print(text)
        return
    
    word_infos, _ = results[0]
    print(format_word_info_text(session, word_infos, include_romanization=False))


def init_db_command(args) -> int:
    """Initialize the himotoki database."""
    import os
    
    # Determine paths
    if args.jmdict:
        jmdict_path = Path(args.jmdict)
    else:
        # Check common locations
        for p in [Path('data/JMdict_e.xml'), Path('JMdict_e.xml')]:
            if p.exists():
                jmdict_path = p
                break
        else:
            print("Error: JMdict file not found.", file=sys.stderr)
            print("Download from: http://ftp.edrdg.org/pub/Nihongo/JMdict_e.gz", file=sys.stderr)
            print("Or specify path with --jmdict", file=sys.stderr)
            return 1
    
    if args.output:
        db_path = Path(args.output)
    else:
        # Default to data directory
        db_path = Path('data/himotoki.db')
    
    if not jmdict_path.exists():
        print(f"Error: JMdict file not found: {jmdict_path}", file=sys.stderr)
        return 1
    
    # Confirm overwrite
    if db_path.exists() and not args.force:
        print(f"Database already exists: {db_path}")
        response = input("Overwrite? [y/N]: ").strip().lower()
        if response != 'y':
            print("Aborted.")
            return 1
    
    db_path.parent.mkdir(parents=True, exist_ok=True)
    
    print(f"Initializing database...")
    print(f"  JMdict: {jmdict_path}")
    print(f"  Output: {db_path}")
    print()
    
    from himotoki.loading.jmdict import load_jmdict
    
    t0 = time.perf_counter()
    
    def progress(count):
        if count % 50000 == 0:
            print(f"  {count:,} entries loaded...")
    
    try:
        total = load_jmdict(
            xml_path=str(jmdict_path),
            db_path=str(db_path),
            load_extras=True,
            batch_size=5000,
            progress_callback=progress
        )
        
        elapsed = time.perf_counter() - t0
        db_size = os.path.getsize(db_path) / 1024 / 1024
        
        print()
        print(f"✅ Database initialized successfully!")
        print(f"   Entries: {total:,}")
        print(f"   Time: {elapsed:.1f}s ({elapsed/60:.1f} min)")
        print(f"   Size: {db_size:.1f}MB")
        print()
        print("Set HIMOTOKI_DB environment variable to use this database:")
        print(f'  export HIMOTOKI_DB="{db_path.absolute()}"')
        
        return 0
        
    except Exception as e:
        print(f"Error initializing database: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()
        return 1


def main_init_db(args: list) -> int:
    """CLI entry point for init-db subcommand."""
    parser = argparse.ArgumentParser(
        description='Initialize the himotoki database from JMdict',
        prog='himotoki init-db',
    )
    
    parser.add_argument(
        '--jmdict', '-j',
        type=str,
        metavar='PATH',
        help='Path to JMdict XML file (default: data/JMdict_e.xml)',
    )
    
    parser.add_argument(
        '--output', '-o',
        type=str,
        metavar='PATH',
        help='Output database path (default: data/himotoki.db)',
    )
    
    parser.add_argument(
        '--force', '-f',
        action='store_true',
        help='Overwrite existing database without prompting',
    )
    
    parsed = parser.parse_args(args)
    return init_db_command(parsed)


def main_setup(args: list) -> int:
    """CLI entry point for setup subcommand."""
    import argparse as ap
    
    parser = ap.ArgumentParser(
        description='Set up the himotoki database',
        prog='himotoki setup',
    )
    
    parser.add_argument(
        '--yes', '-y',
        action='store_true',
        help='Skip confirmation prompt',
    )
    
    parser.add_argument(
        '--force', '-f',
        action='store_true',
        help='Force rebuild even if database exists',
    )
    
    parsed = parser.parse_args(args)
    
    from himotoki.setup import run_setup, is_database_ready, get_db_path, prompt_for_setup
    
    # Check if already set up
    if is_database_ready() and not parsed.force:
        print(f"✅ Database already exists at: {get_db_path()}")
        print("   Use --force to rebuild.")
        return 0
    
    # Prompt for confirmation unless --yes
    if not parsed.yes:
        if not prompt_for_setup():
            return 1
        print()
    
    # Run setup
    success = run_setup(force=parsed.force, confirm=False)
    return 0 if success else 1


def main(args: Optional[list] = None) -> int:
    """Main CLI entry point."""
    # Check for subcommands
    args_list = args if args is not None else sys.argv[1:]
    
    if args_list and args_list[0] == 'setup':
        return main_setup(args_list[1:])
    if args_list and args_list[0] == 'init-db':
        return main_init_db(args_list[1:])
    
    parser = argparse.ArgumentParser(
        description='Command line interface for Himotoki (Japanese Morphological Analyzer)',
        prog='himotoki',
        epilog='Subcommands:\n  himotoki setup      Set up the dictionary database (recommended)\n  himotoki init-db    Initialize database from local JMdict file',
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    
    parser.add_argument(
        'text',
        nargs='*',
        help='Japanese text to analyze',
    )
    
    # Output format flags (mutually exclusive)
    output_group = parser.add_mutually_exclusive_group()
    output_group.add_argument(
        '-r', '--romanize',
        action='store_true',
        help='Simple romanization output only',
    )
    output_group.add_argument(
        '-f', '--full',
        action='store_true',
        help='Full output with romanization and dictionary info',
    )
    output_group.add_argument(
        '-k', '--kana',
        action='store_true',
        help='Kana reading with spaces between words',
    )
    output_group.add_argument(
        '-j', '--json',
        action='store_true',
        help='Full split info as JSON',
    )
    
    parser.add_argument(
        '-l', '--limit',
        type=int,
        default=1,
        metavar='N',
        help='Limit segmentations to N results (default: 1, use with -j)',
    )
    
    parser.add_argument(
        '-d', '--database',
        type=str,
        default=None,
        metavar='PATH',
        help='Path to SQLite database file',
    )
    
    parser.add_argument(
        '-v', '--version',
        action='store_true',
        help='Show version information',
    )
    
    parsed = parser.parse_args(args)
    
    if parsed.version:
        print(f'himotoki {VERSION}')
        return 0
    
    # Get input text
    text = ' '.join(parsed.text) if parsed.text else ''
    
    if not text or not text.strip():
        parser.print_help()
        return 1
    
    # Get database session - with first-use setup prompt
    db_path = parsed.database
    if db_path is None:
        db_path = get_db_path()
    
    if not db_path or not Path(db_path).exists():
        # First-use experience: prompt for setup
        from himotoki.setup import ensure_database_or_prompt, get_db_path as setup_get_db_path
        
        if not ensure_database_or_prompt():
            print("\nRun 'himotoki setup' when you're ready to initialize the database.", file=sys.stderr)
            return 1
        
        # After setup, get the new database path
        db_path = str(setup_get_db_path())
    
    try:
        session = get_session(db_path)
    except Exception as e:
        print(f'Error connecting to database: {e}', file=sys.stderr)
        return 1
    
    # Initialize suffix cache for compound word detection
    from himotoki.suffixes import init_suffixes
    init_suffixes(session)
    
    try:
        # Determine output mode and dispatch (order-independent)
        output_mode = 'default'
        if parsed.json:
            output_mode = 'json'
            limit = parsed.limit if parsed.limit > 0 else 5
            output_json(session, text, limit)
        else:
            # Map flags to handlers (only one can be True due to mutually exclusive group)
            handlers = {
                'romanize': output_romanize,
                'full': output_full,
                'kana': output_kana,
            }
            for flag, fn in handlers.items():
                if getattr(parsed, flag):
                    output_mode = flag
                    fn(session, text)
                    break
            else:
                output_default(session, text)
        
        return 0
    
    except Exception as e:
        print(f'Error processing text (mode={output_mode}): {e}', file=sys.stderr)
        import traceback
        traceback.print_exc()
        return 1
    finally:
        session.close()


if __name__ == '__main__':
    sys.exit(main())

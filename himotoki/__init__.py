"""
Himotoki: Japanese Morphological Analyzer
A Python port of ichiran (https://github.com/tshatrov/ichiran)
"""

import asyncio
import logging
import os
import time
import unicodedata
from concurrent.futures import ThreadPoolExecutor, TimeoutError as FuturesTimeoutError
from contextlib import contextmanager
from typing import Optional, Tuple, List, Any, Generator

__version__ = "0.3.2"

# Logger for himotoki operations (users can attach handlers for request tracing)
logger = logging.getLogger("himotoki")

# =============================================================================
# Configuration Constants
# =============================================================================

# Maximum allowed text length to prevent DoS attacks
# Override with HIMOTOKI_MAX_TEXT_LENGTH environment variable
MAX_TEXT_LENGTH: int = int(os.environ.get("HIMOTOKI_MAX_TEXT_LENGTH", 100))

# Default timeout for analysis (seconds)
# Override with HIMOTOKI_DEFAULT_TIMEOUT environment variable
DEFAULT_TIMEOUT: float = float(os.environ.get("HIMOTOKI_DEFAULT_TIMEOUT", 30.0))

# Thread pool for async operations (SQLite is not async-native)
_executor: Optional[ThreadPoolExecutor] = None


class AnalysisTimeoutError(Exception):
    """Raised when text analysis exceeds the timeout limit."""
    pass


class TextTooLongError(ValueError):
    """Raised when input text exceeds MAX_TEXT_LENGTH."""
    pass


def _get_executor() -> ThreadPoolExecutor:
    """Get or create the thread pool executor for async operations."""
    global _executor
    if _executor is None:
        _executor = ThreadPoolExecutor(max_workers=4, thread_name_prefix="himotoki")
    return _executor


def warm_up(verbose: bool = False) -> Tuple[float, dict]:
    """
    Pre-initialize all caches for optimal performance.
    
    Call this once at application startup to avoid cold-start latency.
    This eagerly builds all lazy-initialized caches:
    - Archaic word cache (~165ms)
    - Suffix pattern cache (~145ms)
    - Counter cache (~10ms)
    
    Args:
        verbose: If True, print timing information.
        
    Returns:
        Tuple of (total_time_seconds, timing_details_dict)
    
    Example:
        >>> import himotoki
        >>> elapsed, details = himotoki.warm_up(verbose=True)
        Warming up himotoki caches...
          Session:        12.3ms
          Archaic cache:  165.4ms
          Suffix cache:   143.2ms
          Counter cache:   10.1ms
        Total warm-up:    330.9ms
    """
    from himotoki.db.connection import get_session
    from himotoki.lookup import build_archaic_cache, _ARCHAIC_CACHE
    from himotoki.suffixes import init_suffixes, is_suffix_cache_ready
    from himotoki.counters import init_counter_cache
    from himotoki.trie import init_word_trie, is_trie_ready, get_trie_size
    
    timings = {}
    total_start = time.perf_counter()
    
    if verbose:
        print("Warming up himotoki caches...")
    
    # 1. Initialize session
    t0 = time.perf_counter()
    session = get_session()
    timings['session'] = (time.perf_counter() - t0) * 1000
    if verbose:
        print(f"  Session:        {timings['session']:>7.1f}ms")
    
    # 2. Build archaic cache (largest cache)
    t0 = time.perf_counter()
    import himotoki.lookup as lookup_module
    if lookup_module._ARCHAIC_CACHE is None:
        lookup_module._ARCHAIC_CACHE = build_archaic_cache(session)
    timings['archaic'] = (time.perf_counter() - t0) * 1000
    if verbose:
        print(f"  Archaic cache:  {timings['archaic']:>7.1f}ms")
    
    # 3. Build suffix cache
    t0 = time.perf_counter()
    if not is_suffix_cache_ready():
        init_suffixes(session)
    timings['suffix'] = (time.perf_counter() - t0) * 1000
    if verbose:
        print(f"  Suffix cache:   {timings['suffix']:>7.1f}ms")
    
    # 4. Build counter cache
    t0 = time.perf_counter()
    init_counter_cache(session)
    timings['counter'] = (time.perf_counter() - t0) * 1000
    if verbose:
        print(f"  Counter cache:  {timings['counter']:>7.1f}ms")
    
    # 5. Build word trie (for fast substring filtering)
    t0 = time.perf_counter()
    if not is_trie_ready():
        init_word_trie(session)
    timings['word_trie'] = (time.perf_counter() - t0) * 1000
    if verbose:
        print(f"  Word trie:      {timings['word_trie']:>7.1f}ms ({get_trie_size():,} entries)")
    
    total_time = time.perf_counter() - total_start
    timings['total'] = total_time * 1000
    
    if verbose:
        print(f"Total warm-up:    {timings['total']:>7.1f}ms")
    
    # Close session if we created it
    session.close()
    
    return total_time, timings


@contextmanager
def session_context() -> Generator[Any, None, None]:
    """
    Context manager for database sessions.
    
    Use this when you need to make multiple calls with the same session.
    The session is automatically closed when exiting the context.
    
    Example:
        >>> with himotoki.session_context() as session:
        ...     result1 = himotoki.analyze("こんにちは", session=session)
        ...     result2 = himotoki.analyze("ありがとう", session=session)
    """
    from himotoki.db.connection import get_session
    session = get_session()
    try:
        yield session
    finally:
        session.close()


def analyze(
    text: str,
    limit: int = 1,
    session: Optional[Any] = None,
    max_length: Optional[int] = None,
) -> List[Tuple[List[Any], int]]:
    """
    Analyze Japanese text and return segmentation results.
    
    This is the main high-level API for text analysis.
    
    Args:
        text: Japanese text to analyze (must be non-empty).
        limit: Maximum number of segmentation results to return (must be >= 1).
        session: Optional database session. If None, creates and auto-closes one.
        max_length: Maximum allowed text length. Defaults to MAX_TEXT_LENGTH (10000).
        
    Returns:
        List of (word_info_list, score) tuples, sorted by score descending.
        
    Raises:
        ValueError: If text is empty or whitespace-only, or limit < 1.
        TextTooLongError: If text exceeds max_length.
        
    Example:
        >>> import himotoki
        >>> himotoki.warm_up()  # Optional but recommended
        >>> results = himotoki.analyze("今日は天気がいい")
        >>> for words, score in results:
        ...     for w in words:
        ...         print(f"{w.text} [{w.kana}]")
        
    Note:
        If you don't provide a session, one will be created and automatically
        closed after the call. For batch processing, use session_context():
        
        >>> with himotoki.session_context() as session:
        ...     for text in texts:
        ...         results = himotoki.analyze(text, session=session)
    """
    # Input validation
    if not text or not text.strip():
        raise ValueError("text must be non-empty and not whitespace-only")
    if limit < 1:
        raise ValueError("limit must be >= 1")
    
    # Unicode normalization - NFC ensures decomposed characters (e.g., か + ゛)
    # are converted to their composed form (e.g., が) for correct dictionary lookup.
    text = unicodedata.normalize('NFC', text)
    
    # Length validation (protection against DoS)
    effective_max = max_length if max_length is not None else MAX_TEXT_LENGTH
    if len(text) > effective_max:
        raise TextTooLongError(
            f"text length ({len(text)}) exceeds maximum allowed ({effective_max}). "
            f"Split your text into smaller chunks."
        )
    
    from himotoki.db.connection import get_session as _get_session
    from himotoki.segment import segment_text
    from himotoki.output import fill_segment_path
    
    # Track if we created the session (so we know to close it)
    created_session = session is None
    if created_session:
        session = _get_session()
    
    try:
        results = segment_text(session, text, limit=limit)
        
        output = []
        for path, score in results:
            words = fill_segment_path(session, text, path)
            output.append((words, score))
        
        return output
    finally:
        # Close session only if we created it
        if created_session:
            session.close()


async def analyze_async(
    text: str,
    limit: int = 1,
    timeout: Optional[float] = None,
) -> List[Tuple[List[Any], int]]:
    """
    Async version of analyze() for use with FastAPI/asyncio.
    
    This runs the synchronous analyze() in a thread pool to avoid
    blocking the event loop. SQLite doesn't support true async, but
    this prevents blocking the main thread.
    
    Args:
        text: Japanese text to analyze (must be non-empty).
        limit: Maximum number of segmentation results to return (must be >= 1).
        timeout: Maximum time to wait for analysis (seconds). 
                 Defaults to DEFAULT_TIMEOUT (30s). Set to None for no timeout.
        
    Returns:
        List of (word_info_list, score) tuples, sorted by score descending.
        
    Raises:
        ValueError: If text is empty or whitespace-only, or limit < 1.
        TextTooLongError: If text exceeds MAX_TEXT_LENGTH.
        AnalysisTimeoutError: If analysis exceeds timeout.
        
    Example (FastAPI):
        from fastapi import FastAPI, HTTPException
        import himotoki
        
        app = FastAPI()
        
        @app.on_event("startup")
        async def startup():
            himotoki.warm_up()
        
        @app.get("/analyze")
        async def analyze_endpoint(text: str, limit: int = 1):
            try:
                results = await himotoki.analyze_async(text, limit=limit, timeout=10.0)
                return himotoki.AnalysisResult.from_analysis_single(results)
            except himotoki.TextTooLongError:
                raise HTTPException(400, "Text too long")
            except himotoki.AnalysisTimeoutError:
                raise HTTPException(408, "Analysis timed out")
    """
    # Do validation before submitting to thread pool for faster error response
    if not text or not text.strip():
        raise ValueError("text must be non-empty and not whitespace-only")
    if limit < 1:
        raise ValueError("limit must be >= 1")
    
    # Unicode normalization - NFC ensures decomposed characters are handled correctly
    text = unicodedata.normalize('NFC', text)
    
    if len(text) > MAX_TEXT_LENGTH:
        raise TextTooLongError(
            f"text length ({len(text)}) exceeds maximum allowed ({MAX_TEXT_LENGTH})"
        )
    
    loop = asyncio.get_event_loop()
    executor = _get_executor()
    
    effective_timeout = timeout if timeout is not None else DEFAULT_TIMEOUT
    
    try:
        # Run synchronous analyze in thread pool with timeout
        if effective_timeout > 0:
            return await asyncio.wait_for(
                loop.run_in_executor(
                    executor,
                    lambda: analyze(text, limit=limit)
                ),
                timeout=effective_timeout
            )
        else:
            # No timeout
            return await loop.run_in_executor(
                executor,
                lambda: analyze(text, limit=limit)
            )
    except asyncio.TimeoutError:
        raise AnalysisTimeoutError(
            f"Analysis timed out after {effective_timeout}s. "
            f"Try with shorter text or increase timeout."
        )


def shutdown():
    """
    Shutdown himotoki and cleanup resources.
    
    Call this when your application is shutting down to properly
    cleanup the thread pool and database connections.
    
    Example (FastAPI):
        @app.on_event("shutdown")
        async def shutdown_event():
            himotoki.shutdown()
    """
    global _executor
    
    # Shutdown thread pool
    if _executor is not None:
        _executor.shutdown(wait=True)
        _executor = None

    # Drop scoring caches so a later restart cannot reuse stale ORM objects
    from himotoki.scoring.caches import clear_scoring_caches
    clear_scoring_caches()
    try:
        from himotoki.output.meanings import clear_meanings_cache
        clear_meanings_cache()
    except Exception:
        pass
    
    # Close database connection
    from himotoki.db.connection import close_connection
    close_connection()
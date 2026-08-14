"""
Tests for himotoki.loading module.
Tests JMDict XML parsing and conjugation loading.
"""

import pytest
from pathlib import Path
import tempfile
import os

from himotoki.db.connection import init_database, session_scope, close_connection
from himotoki.db.models import Entry, KanjiText, KanaText, Sense, Gloss, SenseProp


# Get test data directory
TEST_DATA_DIR = Path(__file__).parent / "data"


@pytest.fixture(scope="function")
def test_db():
    """Create a temporary test database for each test."""
    with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
        db_path = f.name
    
    init_database(db_path, drop_existing=True)
    
    yield db_path
    
    close_connection()
    os.unlink(db_path)


@pytest.fixture(scope="module")
def csv_data_path():
    """Path to test CSV data files."""
    return TEST_DATA_DIR


class TestConjugationCSVLoading:
    """Test loading conjugation data from CSV files."""
    
    def test_load_pos_index(self, csv_data_path):
        """Test loading part of speech index from kwpos.csv."""
        from himotoki.loading.conjugations import load_pos_index, _pos_index, _pos_by_index
        
        # Reset cached data
        import himotoki.loading.conjugations as conj_module
        conj_module._pos_index = None
        conj_module._pos_by_index = None
        
        pos_index = load_pos_index(csv_data_path / "kwpos.csv")
        
        # Check some known POS entries
        assert "n" in pos_index
        assert "v1" in pos_index
        assert "adj-i" in pos_index
        
        # Check the structure (id, description)
        n_entry = pos_index["n"]
        assert isinstance(n_entry, tuple)
        assert n_entry[0] == 23  # id for 'n'
        
        v1_entry = pos_index["v1"]
        assert v1_entry[0] == 36  # id for 'v1'
    
    def test_get_pos_index(self, csv_data_path):
        """Test getting POS ID by name."""
        from himotoki.loading.conjugations import get_pos_index, load_pos_index
        
        # Reset and reload
        import himotoki.loading.conjugations as conj_module
        conj_module._pos_index = None
        load_pos_index(csv_data_path / "kwpos.csv")
        
        assert get_pos_index("v1") == 36
        assert get_pos_index("v5r") == 77
        assert get_pos_index("adj-i") == 2
        assert get_pos_index("vk") == 85  # kuru verb
        assert get_pos_index("nonexistent") is None
    
    def test_get_pos_by_index(self, csv_data_path):
        """Test getting POS name by ID."""
        from himotoki.loading.conjugations import get_pos_by_index, load_pos_index
        
        # Reset and reload
        import himotoki.loading.conjugations as conj_module
        conj_module._pos_by_index = None
        conj_module._pos_index = None
        load_pos_index(csv_data_path / "kwpos.csv")
        
        assert get_pos_by_index(36) == "v1"
        assert get_pos_by_index(77) == "v5r"
        assert get_pos_by_index(2) == "adj-i"
        assert get_pos_by_index(9999) is None
    
    def test_load_conj_descriptions(self, csv_data_path):
        """Test loading conjugation descriptions from conj.csv."""
        from himotoki.loading.conjugations import load_conj_descriptions
        
        # Reset cached data
        import himotoki.loading.conjugations as conj_module
        conj_module._conj_descriptions = None
        
        descriptions = load_conj_descriptions(csv_data_path / "conj.csv")
        
        assert 1 in descriptions
        assert descriptions[1] == "Non-past"
        assert descriptions[2] == "Past (~ta)"
        assert descriptions[3] == "Conjunctive (~te)"
        assert descriptions[5] == "Potential"
    
    def test_load_conj_rules(self, csv_data_path):
        """Test loading conjugation rules from conjo.csv."""
        from himotoki.loading.conjugations import load_conj_rules, ConjugationRule
        
        # Reset cached data
        import himotoki.loading.conjugations as conj_module
        conj_module._conj_rules = None
        
        rules = load_conj_rules(csv_data_path / "conjo.csv")
        
        # Check v1 (Ichidan) rules exist
        assert 36 in rules
        v1_rules = rules[36]
        assert len(v1_rules) > 0
        
        # Check structure
        rule = v1_rules[0]
        assert isinstance(rule, ConjugationRule)
        assert rule.pos == 36
        assert hasattr(rule, 'conj')
        assert hasattr(rule, 'neg')
        assert hasattr(rule, 'fml')
        assert hasattr(rule, 'okuri')
        
        # Check v5r (Godan ru) rules exist
        assert 77 in rules
        
        # Check adj-i rules exist
        assert 2 in rules


class TestConjugationGeneration:
    """Test conjugation generation functions."""
    
    def test_is_kana(self):
        """Test kana detection."""
        from himotoki.loading.conjugations import is_kana
        
        assert is_kana("たべる") == True
        assert is_kana("はしる") == True
        assert is_kana("くる") == True
        assert is_kana("食べる") == False
        assert is_kana("走る") == False
        assert is_kana("") == False
    
    def test_construct_conjugation_v1(self, csv_data_path):
        """Test conjugation construction for Ichidan verbs."""
        from himotoki.loading.conjugations import (
            construct_conjugation, load_conj_rules, load_pos_index, ConjugationRule
        )
        
        # Reset and load
        import himotoki.loading.conjugations as conj_module
        conj_module._conj_rules = None
        conj_module._pos_index = None
        load_pos_index(csv_data_path / "kwpos.csv")
        load_conj_rules(csv_data_path / "conjo.csv")
        
        # たべる (taberu) - Ichidan verb
        # Non-past affirmative: たべる -> たべる
        rule_nonpast = ConjugationRule(pos=36, conj=1, neg=False, fml=False, onum=1, 
                                        stem=1, okuri="る", euphr="", euphk="")
        assert construct_conjugation("たべる", rule_nonpast) == "たべる"
        
        # Non-past negative: たべる -> たべない
        rule_nonpast_neg = ConjugationRule(pos=36, conj=1, neg=True, fml=False, onum=1,
                                            stem=1, okuri="ない", euphr="", euphk="")
        assert construct_conjugation("たべる", rule_nonpast_neg) == "たべない"
        
        # Past affirmative: たべる -> たべた
        rule_past = ConjugationRule(pos=36, conj=2, neg=False, fml=False, onum=1,
                                     stem=1, okuri="た", euphr="", euphk="")
        assert construct_conjugation("たべる", rule_past) == "たべた"
        
        # Te-form: たべる -> たべて
        rule_te = ConjugationRule(pos=36, conj=3, neg=False, fml=False, onum=1,
                                   stem=1, okuri="て", euphr="", euphk="")
        assert construct_conjugation("たべる", rule_te) == "たべて"
    
    def test_construct_conjugation_v5r(self, csv_data_path):
        """Test conjugation construction for Godan ru verbs."""
        from himotoki.loading.conjugations import construct_conjugation, ConjugationRule
        
        # はしる (hashiru) - Godan ru verb
        # Non-past affirmative: はしる -> はしる
        rule_nonpast = ConjugationRule(pos=77, conj=1, neg=False, fml=False, onum=1,
                                        stem=1, okuri="る", euphr="", euphk="")
        assert construct_conjugation("はしる", rule_nonpast) == "はしる"
        
        # Non-past negative: はしる -> はしらない
        rule_neg = ConjugationRule(pos=77, conj=1, neg=True, fml=False, onum=1,
                                    stem=1, okuri="らない", euphr="", euphk="")
        assert construct_conjugation("はしる", rule_neg) == "はしらない"
        
        # Past for Godan ru: はしる -> はしった
        # For v5r, the gemination (っ) is NOT a euphonic change - it's part of okuri
        # The っ replaces the stem る, so stem=1, okuri=った, no euphr needed
        rule_past = ConjugationRule(pos=77, conj=2, neg=False, fml=False, onum=1,
                                     stem=1, okuri="った", euphr="", euphk="")
        assert construct_conjugation("はしる", rule_past) == "はしった"
        
        # Te-form for Godan ru: はしる -> はしって
        rule_te = ConjugationRule(pos=77, conj=3, neg=False, fml=False, onum=1,
                                   stem=1, okuri="って", euphr="", euphk="")
        assert construct_conjugation("はしる", rule_te) == "はしって"
    
    def test_construct_conjugation_adj_i(self, csv_data_path):
        """Test conjugation construction for i-adjectives."""
        from himotoki.loading.conjugations import construct_conjugation, ConjugationRule
        
        # あかい (akai) - i-adjective
        # Non-past affirmative: あかい -> あかい
        rule_nonpast = ConjugationRule(pos=2, conj=1, neg=False, fml=False, onum=1,
                                        stem=1, okuri="い", euphr="", euphk="")
        assert construct_conjugation("あかい", rule_nonpast) == "あかい"
        
        # Non-past negative: あかい -> あかくない
        rule_neg = ConjugationRule(pos=2, conj=1, neg=True, fml=False, onum=1,
                                    stem=1, okuri="くない", euphr="", euphk="")
        assert construct_conjugation("あかい", rule_neg) == "あかくない"
        
        # Past affirmative: あかい -> あかかった
        rule_past = ConjugationRule(pos=2, conj=2, neg=False, fml=False, onum=1,
                                     stem=1, okuri="かった", euphr="", euphk="")
        assert construct_conjugation("あかい", rule_past) == "あかかった"
    
    def test_conjugate_word(self, csv_data_path):
        """Test conjugate_word function."""
        from himotoki.loading.conjugations import conjugate_word, load_conj_rules, load_pos_index
        
        # Reset and load
        import himotoki.loading.conjugations as conj_module
        conj_module._conj_rules = None
        conj_module._pos_index = None
        load_pos_index(csv_data_path / "kwpos.csv")
        load_conj_rules(csv_data_path / "conjo.csv")
        
        # Get all conjugations for たべる
        conjugations = conjugate_word("たべる", "v1")
        assert len(conjugations) > 0
        
        # Find specific conjugations
        conj_texts = [text for _, text in conjugations]
        assert "たべる" in conj_texts  # Non-past
        assert "たべない" in conj_texts  # Negative
        assert "たべた" in conj_texts  # Past
        assert "たべて" in conj_texts  # Te-form


class TestJMDictXMLParsing:
    """Test JMDict XML parsing."""
    
    def test_parse_reading(self):
        """Test parsing individual readings."""
        from himotoki.loading.jmdict import parse_reading
        from lxml import etree
        
        # Create a sample k_ele element
        k_ele = etree.fromstring("""
        <k_ele>
            <keb>学校</keb>
            <ke_pri>ichi1</ke_pri>
            <ke_pri>news1</ke_pri>
            <ke_pri>nf01</ke_pri>
        </k_ele>
        """)
        
        reading = parse_reading(k_ele, 'keb', 'ke_pri')
        
        assert reading.text == "学校"
        assert reading.common == 1  # from nf01
        assert reading.nokanji == False
        assert "[ichi1]" in reading.pri_tags
        assert reading.skip == False
    
    def test_parse_kana_reading_with_nokanji(self):
        """Test parsing kana reading with nokanji marker."""
        from himotoki.loading.jmdict import parse_reading
        from lxml import etree
        
        r_ele = etree.fromstring("""
        <r_ele>
            <reb>する</reb>
            <re_nokanji/>
            <re_pri>ichi1</re_pri>
        </r_ele>
        """)
        
        reading = parse_reading(r_ele, 'reb', 're_pri')
        
        assert reading.text == "する"
        assert reading.nokanji == True
    
    def test_parse_reading_with_restriction(self):
        """Test parsing reading with re_restr."""
        from himotoki.loading.jmdict import parse_reading
        from lxml import etree
        
        r_ele = etree.fromstring("""
        <r_ele>
            <reb>りゅう</reb>
            <re_restr>竜</re_restr>
        </r_ele>
        """)
        
        reading = parse_reading(r_ele, 'reb', 're_pri')
        
        assert reading.text == "りゅう"
        assert "竜" in reading.restrictions
    
    def test_node_text(self):
        """Test text extraction from XML nodes."""
        from himotoki.loading.jmdict import node_text
        from lxml import etree
        
        elem = etree.fromstring("<test>hello world</test>")
        assert node_text(elem) == "hello world"
        
        # Nested elements
        elem = etree.fromstring("<test>hello <b>world</b></test>")
        assert node_text(elem) == "hello world"
    
    def test_load_entry_from_xml(self, test_db):
        """Test loading a single entry from XML."""
        from himotoki.loading.jmdict import load_entry
        from lxml import etree
        
        entry_xml = """
        <entry>
            <ent_seq>1000000</ent_seq>
            <k_ele>
                <keb>学校</keb>
                <ke_pri>ichi1</ke_pri>
                <ke_pri>nf01</ke_pri>
            </k_ele>
            <r_ele>
                <reb>がっこう</reb>
                <re_pri>ichi1</re_pri>
                <re_pri>nf01</re_pri>
            </r_ele>
            <sense>
                <pos>n</pos>
                <gloss>school</gloss>
            </sense>
        </entry>
        """
        
        entry_elem = etree.fromstring(entry_xml)
        
        with session_scope() as session:
            seq = load_entry(session, entry_elem)
            session.commit()
            
            assert seq == 1000000
            
            # Verify entry created
            entry = session.query(Entry).filter(Entry.seq == 1000000).first()
            assert entry is not None
            assert entry.root_p == True
            
            # Verify kanji reading
            kanji = session.query(KanjiText).filter(KanjiText.seq == 1000000).first()
            assert kanji is not None
            assert kanji.text == "学校"
            assert kanji.common == 1
            
            # Verify kana reading
            kana = session.query(KanaText).filter(KanaText.seq == 1000000).first()
            assert kana is not None
            assert kana.text == "がっこう"
            
            # Verify sense
            sense = session.query(Sense).filter(Sense.seq == 1000000).first()
            assert sense is not None
            
            # Verify gloss
            gloss = session.query(Gloss).filter(Gloss.sense_id == sense.id).first()
            assert gloss is not None
            assert gloss.text == "school"
            
            # Verify sense property (pos)
            pos = session.query(SenseProp).filter(
                SenseProp.sense_id == sense.id,
                SenseProp.tag == "pos"
            ).first()
            assert pos is not None
            assert pos.text == "n"
    
    def test_load_entry_with_multiple_readings(self, test_db):
        """Test loading entry with multiple kanji and kana readings."""
        from himotoki.loading.jmdict import load_entry
        from lxml import etree
        
        entry_xml = """
        <entry>
            <ent_seq>1000060</ent_seq>
            <k_ele>
                <keb>日本</keb>
                <ke_pri>ichi1</ke_pri>
            </k_ele>
            <k_ele>
                <keb>日本國</keb>
            </k_ele>
            <r_ele>
                <reb>にほん</reb>
                <re_pri>ichi1</re_pri>
            </r_ele>
            <r_ele>
                <reb>にっぽん</reb>
            </r_ele>
            <sense>
                <pos>n</pos>
                <gloss>Japan</gloss>
            </sense>
        </entry>
        """
        
        entry_elem = etree.fromstring(entry_xml)
        
        with session_scope() as session:
            seq = load_entry(session, entry_elem)
            session.commit()
            
            # Verify multiple kanji readings
            kanji_readings = session.query(KanjiText).filter(
                KanjiText.seq == 1000060
            ).order_by(KanjiText.ord).all()
            assert len(kanji_readings) == 2
            assert kanji_readings[0].text == "日本"
            assert kanji_readings[1].text == "日本國"
            
            # Verify multiple kana readings
            kana_readings = session.query(KanaText).filter(
                KanaText.seq == 1000060
            ).order_by(KanaText.ord).all()
            assert len(kana_readings) == 2
            assert kana_readings[0].text == "にほん"
            assert kana_readings[1].text == "にっぽん"
    
    def test_load_entry_with_multiple_senses(self, test_db):
        """Test loading entry with multiple senses."""
        from himotoki.loading.jmdict import load_entry
        from lxml import etree
        
        entry_xml = """
        <entry>
            <ent_seq>1000020</ent_seq>
            <k_ele>
                <keb>赤い</keb>
            </k_ele>
            <r_ele>
                <reb>あかい</reb>
            </r_ele>
            <sense>
                <pos>adj-i</pos>
                <gloss>red</gloss>
                <gloss>crimson</gloss>
            </sense>
            <sense>
                <misc>id</misc>
                <gloss>complete</gloss>
                <gloss>total</gloss>
            </sense>
        </entry>
        """
        
        entry_elem = etree.fromstring(entry_xml)
        
        with session_scope() as session:
            seq = load_entry(session, entry_elem)
            session.commit()
            
            # Verify multiple senses
            senses = session.query(Sense).filter(
                Sense.seq == 1000020
            ).order_by(Sense.ord).all()
            assert len(senses) == 2
            
            # Check first sense has pos
            pos = session.query(SenseProp).filter(
                SenseProp.sense_id == senses[0].id,
                SenseProp.tag == "pos"
            ).first()
            assert pos is not None
            
            # Check first sense has multiple glosses
            glosses1 = session.query(Gloss).filter(
                Gloss.sense_id == senses[0].id
            ).all()
            assert len(glosses1) == 2
            
            # Check second sense has misc
            misc = session.query(SenseProp).filter(
                SenseProp.sense_id == senses[1].id,
                SenseProp.tag == "misc"
            ).first()
            assert misc is not None
            assert misc.text == "id"
    
    def test_iter_entries(self):
        """Test iterating over entries in XML file."""
        from himotoki.loading.jmdict import iter_entries
        
        xml_path = TEST_DATA_DIR / "sample_jmdict.xml"
        
        entries = list(iter_entries(xml_path))
        assert len(entries) == 8  # 8 entries in sample file
    
    def test_load_jmdict_sample(self, test_db):
        """Test loading sample JMDict file."""
        from himotoki.loading.jmdict import load_jmdict
        
        xml_path = TEST_DATA_DIR / "sample_jmdict.xml"
        
        count = load_jmdict(xml_path, load_extras=False)
        
        assert count == 8  # 8 entries in sample
        
        # Verify data loaded correctly
        with session_scope() as session:
            entries = session.query(Entry).all()
            assert len(entries) == 8
            
            # Check specific entry
            gakkou = session.query(Entry).filter(Entry.seq == 1000000).first()
            assert gakkou is not None
            
            kanji = session.query(KanjiText).filter(KanjiText.seq == 1000000).first()
            assert kanji.text == "学校"


class TestPrefetchEntryData:
    """Tests for _prefetch_entry_data batching."""
    
    def test_prefetch_entry_data_batches_large_seq_lists(self, test_db, csv_data_path):
        """Ensure _prefetch_entry_data splits large IN clauses to stay under SQLite's variable limit."""
        from himotoki.loading.conjugations import _prefetch_entry_data, _SQLITE_MAX_VARS_PER_QUERY
        from himotoki.loading.jmdict import load_entry
        from lxml import etree
        
        entry_xml = """
        <entry>
            <ent_seq>2000010</ent_seq>
            <k_ele><keb>食べる</keb></k_ele>
            <r_ele><reb>たべる</reb></r_ele>
            <sense><pos>v1</pos><gloss>to eat</gloss></sense>
        </entry>
        """
        entry_elem = etree.fromstring(entry_xml)
        
        with session_scope() as session:
            load_entry(session, entry_elem)
            session.commit()
            
            # Force a tiny chunk size to exercise batching without needing thousands of rows.
            import himotoki.loading.conjugations as conj_module
            original_chunk = _SQLITE_MAX_VARS_PER_QUERY
            conj_module._SQLITE_MAX_VARS_PER_QUERY = 2
            
            try:
                # Ask for many non-existent seqs plus the real one. Without batching this
                # would exceed SQLite's 999-parameter limit and raise OperationalError.
                seqs = list(range(1001)) + [2000010]
                result = _prefetch_entry_data(session, seqs)
                
                assert len(result) == len(seqs)
                assert result[2000010]['posi'] == ['v1']
                assert result[2000010]['all_readings'] == {'食べる', 'たべる'}
            finally:
                conj_module._SQLITE_MAX_VARS_PER_QUERY = original_chunk


class TestIntegration:
    """Integration tests for loading pipeline."""
    
    def test_full_loading_pipeline(self, test_db, csv_data_path):
        """Test full loading and conjugation generation pipeline."""
        from himotoki.loading.jmdict import load_jmdict, load_entry
        from himotoki.loading.conjugations import (
            load_pos_index, load_conj_rules, conjugate_word
        )
        from lxml import etree
        
        # Setup conjugation data
        import himotoki.loading.conjugations as conj_module
        conj_module._conj_rules = None
        conj_module._pos_index = None
        load_pos_index(csv_data_path / "kwpos.csv")
        load_conj_rules(csv_data_path / "conjo.csv")
        
        # Load a verb entry
        entry_xml = """
        <entry>
            <ent_seq>2000010</ent_seq>
            <k_ele>
                <keb>食べる</keb>
            </k_ele>
            <r_ele>
                <reb>たべる</reb>
            </r_ele>
            <sense>
                <pos>v1</pos>
                <gloss>to eat</gloss>
            </sense>
        </entry>
        """
        
        entry_elem = etree.fromstring(entry_xml)
        
        with session_scope() as session:
            seq = load_entry(session, entry_elem)
            session.commit()
            
            # Verify entry loaded
            kana = session.query(KanaText).filter(KanaText.seq == 2000010).first()
            assert kana.text == "たべる"
            
            # Get POS for entry
            pos = session.query(SenseProp).filter(
                SenseProp.seq == 2000010,
                SenseProp.tag == "pos"
            ).first()
            assert pos.text == "v1"
            
            # Generate conjugations
            conjugations = conjugate_word(kana.text, pos.text)
            
            # Verify key conjugations
            conj_texts = {text for _, text in conjugations}
            assert "たべない" in conj_texts  # negative
            assert "たべた" in conj_texts    # past
            assert "たべて" in conj_texts    # te-form
            assert "たべます" in conj_texts  # polite
            assert "たべられる" in conj_texts  # potential/passive


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
use crate::{File, OpenOptions, types::compute_concatenate_hash, cacher::CacheManagerOld};
use super::{Page, PAGE_SIZE, MAX_NUM_HASH_IN_PAGE};
use std::os::unix::prelude::FileExt;
use serde::{Serialize, Deserialize};
use primitive_types::H256;

/* A helper that writes the hash into a file with a sequence of pages
   According to the disk-optimization objective, the state writing should be in a streaming fashion.
 */
pub struct HashPageWriterOld {
    pub file: File, // file object of the corresponding Merkle file
    pub vec_in_latest_update_page: Vec<H256>, // a preparation vector to obsorb the streaming state values which are not persisted in the file yet
    pub num_stored_pages: usize, // records the number of pages that are stored in the file
}

impl HashPageWriterOld {
    /* Initialize the writer using a given file name
     */
    pub fn create(file_name: &str) -> Self {
        let file = OpenOptions::new().create(true).read(true).write(true).truncate(true).open(&file_name).unwrap();
        Self {
            file,
            vec_in_latest_update_page: vec![],
            num_stored_pages: 0,
        }
    }

    /* Streamingly add the hash to the latest_update_page
       Flush the latest_update_page to the file once it is full, and clear it.
     */
    pub fn append(&mut self, hash: H256) {
        // add the hash
        self.vec_in_latest_update_page.push(hash);
        if self.vec_in_latest_update_page.len() == MAX_NUM_HASH_IN_PAGE {
            // vector is full, should be added to a page and flushed the page to the file
            self.flush();
        }
    }

    /* Flush the vector in latest update page to the last page in the value file
     */
    pub fn flush(&mut self) {
        if self.vec_in_latest_update_page.len() != 0 {
            // first put the vector into a page
            let page = Page::from_hash_vec(&self.vec_in_latest_update_page);
            // compute the offset at which the page will be written in the file
            let offset = self.num_stored_pages * PAGE_SIZE;
            // write the page to the file
            self.file.write_all_at(&page.block, offset as u64).unwrap();
            // clear the vector
            self.vec_in_latest_update_page.clear();
            self.num_stored_pages += 1;
        }
    }

    /* Transform pager to reader
     */
    pub fn to_hash_reader_old(self) -> HashPageReaderOld {
        let file = self.file;
        let num_stored_pages = file.metadata().unwrap().len() as usize / PAGE_SIZE;
        let mut root = None;
        if num_stored_pages != 0 {
            let last_page_id = num_stored_pages - 1;
            let last_page_offset = last_page_id * PAGE_SIZE;
            // get last page from file
            let mut bytes = [0u8; PAGE_SIZE];
            file.read_exact_at(&mut bytes, last_page_offset as u64).unwrap();
            let page = Page::from_array(bytes);
            let page_vec = page.to_hash_vec();
            root = Some(*page_vec.last().unwrap());
        }
        HashPageReaderOld {
            file,
            root,
        }
    }
}

/* A helper to read hash from the file
   A LRU cache is used to optimize the read performance.
 */
pub struct HashPageReaderOld {
    pub file: File, // file object of the corresponding Merkle file
    pub root: Option<H256>, // cache of the root hash
}

impl HashPageReaderOld {
    /* Load the reader from a given file. 
     */
    pub fn load(file_name: &str) -> Self {
        let file = OpenOptions::new().create(true).read(true).write(true).open(&file_name).unwrap();
        let num_stored_pages = file.metadata().unwrap().len() as usize / PAGE_SIZE;
        let mut root = None;
        if num_stored_pages != 0 {
            let last_page_id = num_stored_pages - 1;
            let last_page_offset = last_page_id * PAGE_SIZE;
            // get last page from file
            let mut bytes = [0u8; PAGE_SIZE];
            file.read_exact_at(&mut bytes, last_page_offset as u64).unwrap();
            let page = Page::from_array(bytes);
            let page_vec = page.to_hash_vec();
            root = Some(*page_vec.last().unwrap());
        }
        HashPageReaderOld {
            file,
            root,
        }
    }

    /* Load the deserialized vector of the page from the file at given page_id
     */
    pub fn read_deser_page_at(&mut self, run_id: u32, page_id: usize, cache_manager: &mut CacheManagerOld) -> Vec<H256> {
        // first check whether the cache contains the page
        let r = cache_manager.read_mht_cache(run_id, page_id);
        if r.is_some() {
            // cache contains the page
            let page = r.unwrap();
            page.to_hash_vec()
        } else {
            // cache does not contain the page, should load the page from the file
            let offset = page_id * PAGE_SIZE;
            let mut v = [0u8; PAGE_SIZE];
            self.file.read_exact_at(&mut v, offset as u64).unwrap();
            let page = Page::from_array(v);
            let v = page.to_hash_vec();
            // before return the vector, add it to the cache with page_id as the key
            cache_manager.set_mht_cache(run_id, page_id, page);
            return v;
        }
    }

    /* Generate the range proof given the left position l, right position r, and the total number of states in the leaf of the MHT
     */
    pub fn prove_mht_file_range(&mut self, run_id: u32, l: usize, r: usize, num_of_data: usize, fanout: usize, cache_manager: &mut CacheManagerOld) -> OldMHTRangeProof {
        let mut proof = OldMHTRangeProof::new();
        proof.index_list = [l, r];
        if num_of_data == 1 {
            // only one data, just return the empty proof since the data's hash equals the root hash
            return proof;
        } else {
            let mut cur_level_size = num_of_data;
            // a position that is used to determine the first position of the current level
            let mut start_idx = 0;
            // compute the level's left and right position
            let mut level_l = l;
            let mut level_r = r;
            // iteratively add the hash values from the bottom to the top
            while cur_level_size != 1 {
                // compute the boundary of the two positions (i.e. used to generate the left and right hashes of the proved Merkle node to reconstruct the Merkle root)
                let proof_pos_l = level_l - level_l % fanout;
                let proof_pos_r = if level_r - level_r % fanout + fanout > cur_level_size {
                    cur_level_size
                } else {
                    level_r - level_r % fanout + fanout
                } - 1;
                let proof_pos_l = proof_pos_l + start_idx;
                let proof_pos_r = proof_pos_r + start_idx;
                // next, retrieve the hash values from the position (proof_pos_l + start_idx) to (proof_pos_r + start_idx)
                // compute the corresponding page id
                let page_id_l = proof_pos_l / MAX_NUM_HASH_IN_PAGE;
                let page_id_r = proof_pos_r / MAX_NUM_HASH_IN_PAGE;
                let mut v = Vec::<H256>::new();
                for page_id in page_id_l ..= page_id_r {
                    let hashes = self.read_deser_page_at(run_id, page_id, cache_manager);
                    v.extend(&hashes);
                }
                // keep the hashes from proof_pos_l % MAX_NUM_HASH_IN_PAGE to 
                let left_slice_pos = proof_pos_l % MAX_NUM_HASH_IN_PAGE;
                let right_slice_pos = (page_id_r - page_id_l) * MAX_NUM_HASH_IN_PAGE + (proof_pos_r % MAX_NUM_HASH_IN_PAGE);
                v = v[left_slice_pos ..= right_slice_pos].to_vec();
                // remove the proving hashes from index level_l - proof_pos_l to level_r - proof_pos_l
                for _ in 0..(level_r - level_l + 1) {
                    v.remove(level_l - (proof_pos_l - start_idx));
                }

                proof.p.push(v);
                level_l /= fanout;
                level_r /= fanout;
                start_idx += cur_level_size;
                cur_level_size = ((cur_level_size as f64) / fanout as f64).ceil() as usize;
            }
            return proof;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OldMHTRangeProof {
    pub index_list: [usize; 2], // include left and right position
    pub p: Vec<Vec<H256>>,
}

impl OldMHTRangeProof {
    pub fn new() -> Self {
        Self {
            index_list: [0, 0],
            p: Vec::new(),
        }
    }
}

pub fn reconstruct_old_mht_range_proof(proof: &OldMHTRangeProof, fanout: usize, obj_hashes: Vec<H256>) -> H256 {
    let l = proof.index_list[0];
    let r = proof.index_list[1];
    let mut index_list: Vec<usize> = (l..=r).collect();
    if index_list.len() == 1 && index_list[0] == 0 && proof.p.len() == 0 {
        return obj_hashes[0].clone();
    } else {
        let mut inserted_hashes = obj_hashes;
        for elem in &proof.p {
            let mut v = elem.clone();
            let offset = index_list[0] % fanout;
            for i in 0..index_list.len() {
                v.insert(i + offset, inserted_hashes[i]);
            }
            inserted_hashes.clear();
            // hash_seg: recomputed hash number for the current level
            let hash_seg = (v.len() as f64 / fanout as f64).ceil() as usize;
            
            for j in 0..hash_seg {
                let start_idx = j * fanout;
                let end_idx = if start_idx + fanout > v.len() {
                    v.len() - 1
                } else {
                    start_idx + fanout - 1
                };
                let sub_hash_vec = &v[start_idx ..= end_idx];
                let h = compute_concatenate_hash(sub_hash_vec);
                inserted_hashes.push(h);
            }
            for index in index_list.iter_mut() {
                (*index) /= fanout;
            }
            index_list.dedup();
        }
        assert_eq!(inserted_hashes.len(), 1);
        return inserted_hashes[0];
    }
}

/* A MHT constructor that generates and appends hashes to the file in a streaming fashion
 */
pub struct StreamMHTConstructor {
    pub output_mht_writer: HashPageWriterOld, // a writer of the output Merkle file
    pub fanout: usize,
    pub num_of_hash: usize, // record the total number of hash values in the file
    pub cnt_in_level: usize, // a counter for the focused level
}

impl StreamMHTConstructor {
    /* Initiate the constructor with the output Merkle file name and the fanout of the MHT
     */
    pub fn new(output_file_name: &str, fanout: usize) -> Self {
        // create the output model writer
        let output_mht_writer = HashPageWriterOld::create(output_file_name);
        Self {
            output_mht_writer,
            fanout,
            num_of_hash: 0,
            cnt_in_level: 0,
        }
    }

    /* Streaminly append the hash to the cache of the lowest level
     */
    pub fn append_hash(&mut self, hash: H256) {
        // add the hash to the output writer
        self.output_mht_writer.append(hash);
        self.num_of_hash += 1;
        self.cnt_in_level += 1;
    }

    pub fn reset_cnt_in_level(&mut self) {
        self.cnt_in_level = 0;
    }

    /* Finalize the append of the state
       Recursively build the MHT upon the lowest level and append them to the file in a streaming fashion.
     */
    pub fn build_mht(&mut self) {
        /*
        recursively construct MHT 
        */
        // n is the number of hash values of the current input MHT level
        let mut n = self.num_of_hash;
        while n != 1 {
            // reset the cnt in the level
            self.reset_cnt_in_level();
            // start_hash_pos is the position of the starting input hash value of the current level
            let start_hash_pos = self.num_of_hash - n;
            // end_hash_pos is the position of the ending input hash value of the current level
            let end_hash_pos = self.num_of_hash - 1;
            let start_page_id = start_hash_pos / MAX_NUM_HASH_IN_PAGE;
            let end_page_id = end_hash_pos / MAX_NUM_HASH_IN_PAGE;
            let mut cache_vec = Vec::<H256>::new();
            for page_id in start_page_id ..= end_page_id {
                let mut page_vec = self.read_page(page_id);
                if page_id == start_page_id {
                    page_vec = page_vec[start_hash_pos % MAX_NUM_HASH_IN_PAGE ..].to_vec();
                } else if page_id == end_page_id {
                    page_vec = page_vec[0..= end_hash_pos % MAX_NUM_HASH_IN_PAGE].to_vec();
                }
                for hash in page_vec {
                    cache_vec.push(hash);
                    if cache_vec.len() == self.fanout {
                        // cache is full, the hash of the concatenated hsah values in the cache should be computed and added to the output_mht_writer
                        let h = compute_concatenate_hash(&cache_vec);
                        self.output_mht_writer.append(h);
                        self.num_of_hash += 1;
                        self.cnt_in_level += 1;
                        cache_vec.clear();
                    }
                }
            }
            if cache_vec.len() != 0 {
                let h = compute_concatenate_hash(&cache_vec);
                self.output_mht_writer.append(h);
                self.num_of_hash += 1;
                self.cnt_in_level += 1;
                cache_vec.clear();
            }
            n = self.cnt_in_level;
        }
        self.output_mht_writer.flush();
    }

    fn read_page(&mut self, page_id: usize) -> Vec<H256> {
        let mut hash_vec = Vec::<H256>::new();
        if page_id >= self.output_mht_writer.num_stored_pages {
            // page should be read from the in-memory cache vector
            for hash in self.output_mht_writer.vec_in_latest_update_page.clone() {
                hash_vec.push(hash);
            }
        } else {
            let offset = page_id * PAGE_SIZE;
            let mut bytes = [0u8; PAGE_SIZE];
            self.output_mht_writer.file.read_exact_at(&mut bytes, offset as u64).unwrap();
            let page = Page::from_array(bytes);
            // deserialize the hashes from the page
            let v = page.to_hash_vec();
            for hash in v {
                hash_vec.push(hash);
            }
        }
        return hash_vec;
    }
}

#[cfg(test)]
mod tests {
    use caches::Cache;
    use rand::{rngs::StdRng, SeedableRng};
    use super::*;

    #[test]
    fn test_hash_pager_old() {
        let n = 1000000;
        let mut rng = StdRng::seed_from_u64(1);
        let mut writer = HashPageWriterOld::create("hash.dat");
        let mut hash_vec = Vec::<H256>::new();
        for _ in 0..n {
            let hash = H256::random_using(&mut rng);
            hash_vec.push(hash.clone());
            writer.append(hash);
        }
        writer.flush();
        let mut reader = writer.to_hash_reader_old();
        let mut cache_manager = CacheManagerOld::new();
        for j in 0..3 {
            // iteratively read the pages
            let start = std::time::Instant::now();
            for i in 0..n {
                let page_id = i / MAX_NUM_HASH_IN_PAGE;
                let inner_page_pos = i % MAX_NUM_HASH_IN_PAGE;
                let v = reader.read_deser_page_at(0, page_id, &mut cache_manager);
                let hash = v[inner_page_pos];
                assert_eq!(hash, hash_vec[i]);
            }
            let elapse = start.elapsed().as_nanos() as usize / n;
            println!("round {}, read hash time: {}", j, elapse);
        }
        println!("size of cache: {}", cache_manager.mht_cache.len());
    }

    #[test]
    fn test_construct_mht() {
        let n = 1000;
        let fanout = 16;
        let start = std::time::Instant::now();
        let mut constructor = StreamMHTConstructor::new("hash.dat", fanout);
        for i in 0..n {
            let hash = H256::from_low_u64_be(i);
            constructor.append_hash(hash);
        }
        constructor.build_mht();
        let elapse = start.elapsed().as_nanos();
        println!("elapse: {}", elapse);
        let mut reader = constructor.output_mht_writer.to_hash_reader_old();
        let mut cache_manager = CacheManagerOld::new();
        let root = reader.root.unwrap();
        println!("root: {:?}", root);
        // let pos = 0;
        // let mut p = reader.prove_non_leaf(pos, pos, n as usize, fanout);
        // println!("{:?}", p);
        let leaf_hash_collection: Vec<H256> = (0..n).map(|i| H256::from_low_u64_be(i)).collect();
        let mut cnt = 0;
        let start = std::time::Instant::now();
        for i in 0..n {
            // println!("i: {}", i);
            for j in i..n {
                let l = i as usize;
                let r = j as usize;
                let p = reader.prove_mht_file_range(0, l, r, n as usize, fanout, &mut cache_manager);
                let obj_hashes = leaf_hash_collection[l ..=r].to_vec();
                let h_re = reconstruct_old_mht_range_proof(&p, fanout, obj_hashes);
                if h_re != root {
                    println!("i: {}, h_re: {:?}, root: {:?}", i, h_re, root);
                }
                cnt += 1;
            }
        }
        let elapse = start.elapsed().as_nanos() / cnt as u128;
        println!("average verify: {}", elapse);
    }
}
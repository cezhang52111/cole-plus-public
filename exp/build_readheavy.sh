~/ycsb-0.17.0/bin/ycsb.sh load basic -P ./readheavy/readheavy-uniform > ./readheavy/readheavy-uniform-load.txt
~/ycsb-0.17.0/bin/ycsb.sh run basic -P ./readheavy/readheavy-uniform > ./readheavy/readheavy-uniform-run.txt
cat ./readheavy/readheavy-uniform-load.txt ./readheavy/readheavy-uniform-run.txt > ./readheavy/readheavy-uniform-data.txt
rm ./readheavy/readheavy-uniform-load.txt ./readheavy/readheavy-uniform-run.txt

~/ycsb-0.17.0/bin/ycsb.sh load basic -P ./readheavy/readheavy-zipfian > ./readheavy/readheavy-zipfian-load.txt
~/ycsb-0.17.0/bin/ycsb.sh run basic -P ./readheavy/readheavy-zipfian > ./readheavy/readheavy-zipfian-run.txt
cat ./readheavy/readheavy-zipfian-load.txt ./readheavy/readheavy-zipfian-run.txt > ./readheavy/readheavy-zipfian-data.txt
rm ./readheavy/readheavy-zipfian-load.txt ./readheavy/readheavy-zipfian-run.txt
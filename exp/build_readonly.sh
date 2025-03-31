~/ycsb-0.17.0/bin/ycsb.sh load basic -P ./readonly/readonly-uniform > ./readonly/readonly-uniform-load.txt
~/ycsb-0.17.0/bin/ycsb.sh run basic -P ./readonly/readonly-uniform > ./readonly/readonly-uniform-run.txt
cat ./readonly/readonly-uniform-load.txt ./readonly/readonly-uniform-run.txt > ./readonly/readonly-uniform-data.txt
rm ./readonly/readonly-uniform-load.txt ./readonly/readonly-uniform-run.txt

~/ycsb-0.17.0/bin/ycsb.sh load basic -P ./readonly/readonly-zipfian > ./readonly/readonly-zipfian-load.txt
~/ycsb-0.17.0/bin/ycsb.sh run basic -P ./readonly/readonly-zipfian > ./readonly/readonly-zipfian-run.txt
cat ./readonly/readonly-zipfian-load.txt ./readonly/readonly-zipfian-run.txt > ./readonly/readonly-zipfian-data.txt
rm ./readonly/readonly-zipfian-load.txt ./readonly/readonly-zipfian-run.txt
__kernel void cuber( __global int* buf ){
    int idx = get_global_id(0);    
    buf[idx] = buf[idx] * buf[idx] * buf[idx];
}

__kernel void squarer( __global int* buf ){
    int idx = get_global_id(0);    
    buf[idx] = buf[idx] * buf[idx];
}

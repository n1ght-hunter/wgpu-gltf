// This type definition is just to make typing a bit easier
alias float4 = vec4<f32>;

struct VertexInput {
    // Input vertex data will in attribute 0 will map to this member
    @location(0) position: float4,
    // Input vertex data will in attribute 1 will map to this member
    @location(1) color: float4,
};

struct VertexOutput {
    // The builtin position attribute is passed the transformed position
    // of our input vertex
    @builtin(position) position: float4,
    // We can pass other attributes through as well
    @location(0) color: float4,
};

@vertex
fn vs_main(vert: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.color = vert.color;
    out.position = vert.position;
    return out;
};

@fragment
fn fs_main(in: VertexOutput) -> @location(0) float4 {
    return float4(in.color);
}
